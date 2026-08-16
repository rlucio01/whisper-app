//! Captura de áudio do microfone padrão.
//!
//! ## Por que uma thread dedicada?
//!
//! O `cpal::Stream` **não é `Send`** no Windows (WASAPI usa thread-local
//! state). Isso significa que não podemos guardá-lo em um `Mutex` compartilhado
//! entre threads. A solução idiomática é criar uma **thread dedicada** que
//! possui o stream durante todo o ciclo de vida, e comunicar com ela via um
//! canal `mpsc` (multi-producer, single-consumer).
//!
//! ## Fluxo
//!
//! ```text
//!   [hotkey handler]  --(Command::Start/Stop)-->  [audio thread]
//!                                                    │
//!                                                    ▼
//!                                              possui o Stream do cpal,
//!                                              acumula samples em memória,
//!                                              escreve WAV ao parar,
//!                                              emite eventos Tauri de volta
//!                                              pra UI (recording-saved / recording-error)
//! ```

use std::path::PathBuf;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use tauri::{AppHandle, Emitter, Manager, Runtime};

use crate::config::SharedConfig;
use crate::transcription::TranscriptionService;
use crate::visual;

/// Comandos que a UI/hotkey envia para a thread de áudio.
enum Command {
    Start,
    Stop,
}

/// Handle público para controlar o serviço de áudio.
/// Guardado como state do Tauri (via `.manage()`) e acessado pelo handler
/// do hotkey global.
pub struct AudioService {
    cmd_tx: mpsc::Sender<Command>,
}

impl AudioService {
    /// Sobe a thread de áudio e retorna um handle para controlá-la.
    ///
    /// A thread fica escutando comandos em loop até o canal ser fechado
    /// (o que só acontece quando o app encerra e o `AudioService` é dropado).
    pub fn spawn<R: Runtime>(app: AppHandle<R>) -> Self {
        let (cmd_tx, cmd_rx) = mpsc::channel::<Command>();

        thread::spawn(move || audio_thread_loop(cmd_rx, app));

        Self { cmd_tx }
    }

    /// Dispara início de gravação. Se já estiver gravando, é ignorado.
    /// Não bloqueia — retorna assim que o comando é enfileirado.
    pub fn start(&self) {
        // `.send()` só falha se o receiver foi dropado (thread morreu). Ignoro
        // porque não tem muito o que fazer nesse caso além de logar.
        let _ = self.cmd_tx.send(Command::Start);
    }

    /// Dispara fim de gravação. Se não estiver gravando, é ignorado.
    pub fn stop(&self) {
        let _ = self.cmd_tx.send(Command::Stop);
    }
}

/// Estado interno da gravação em curso (só existe entre Start e Stop).
struct Recording {
    // Ownership do stream mantido aqui — dropar o stream **para** a gravação.
    _stream: cpal::Stream,
    samples: Arc<Mutex<Vec<f32>>>,
    sample_rate: u32,
    channels: u16,
}

/// Loop principal da thread de áudio.
///
/// Recebe comandos via canal. Se Start, abre o mic e começa a acumular samples.
/// Se Stop, escreve WAV e emite evento Tauri com o path (ou erro).
fn audio_thread_loop<R: Runtime>(cmd_rx: mpsc::Receiver<Command>, app: AppHandle<R>) {
    let mut recording: Option<Recording> = None;

    while let Ok(cmd) = cmd_rx.recv() {
        match cmd {
            Command::Start => {
                if recording.is_some() {
                    // Ignora Start duplicado (usuário martelou o hotkey).
                    continue;
                }
                // Snapshot do device escolhido em settings. Vazio = default do SO.
                let device_name = app
                    .try_state::<SharedConfig>()
                    .and_then(|s| s.lock().ok().map(|g| g.microphone.clone()))
                    .unwrap_or_default();
                match start_recording(&device_name) {
                    Ok(r) => {
                        recording = Some(r);
                    }
                    Err(e) => {
                        let _ = app.emit("recording-error", format!("{:#}", e));
                    }
                }
            }
            Command::Stop => {
                let Some(r) = recording.take() else {
                    // Stop sem Start prévio, ignora.
                    continue;
                };
                match finalize_recording(r) {
                    Ok(path) => {
                        let _ = app
                            .emit("recording-saved", path.to_string_lossy().to_string());
                        // Encadeia direto para a transcrição — o serviço vai
                        // emitir `transcription-complete` (ou error) quando
                        // terminar, e ele mesmo apaga o WAV depois.
                        app.state::<TranscriptionService>().transcribe(path);
                    }
                    Err(e) => {
                        let _ = app.emit("recording-error", format!("{:#}", e));
                        // Erro na gravação (ex: áudio muito curto) = fim do
                        // pipeline; esconde o indicador visual.
                        visual::set(&app, visual::State::Idle);
                    }
                }
            }
        }
    }
}

/// Abre o microfone escolhido (ou o default do SO, se `device_name` vazio) e
/// começa a acumular samples no buffer compartilhado.
fn start_recording(device_name: &str) -> Result<Recording> {
    let host = cpal::default_host();
    let device = if device_name.trim().is_empty() {
        host.default_input_device()
            .ok_or_else(|| anyhow!("nenhum dispositivo de entrada de áudio encontrado"))?
    } else {
        host.input_devices()
            .context("falha ao listar dispositivos de entrada")?
            .find(|d| d.name().map(|n| n == device_name).unwrap_or(false))
            .ok_or_else(|| {
                anyhow!(
                    "microfone \"{}\" não encontrado — verifique se ainda está \
                     conectado, ou escolha outro em Configurações.",
                    device_name
                )
            })?
    };

    let config = device
        .default_input_config()
        .context("falha ao obter config default do microfone")?;

    let sample_rate = config.sample_rate().0;
    let channels = config.channels();
    let sample_format = config.sample_format();

    // Buffer compartilhado entre a callback do cpal (que roda em uma thread
    // interna do driver de áudio) e a nossa thread. `Arc<Mutex<Vec<f32>>>` é o
    // padrão idiomático: `Arc` para múltiplos donos, `Mutex` para acesso
    // sincronizado. Como estamos só empurrando samples, a contenção é mínima.
    let samples = Arc::new(Mutex::new(Vec::<f32>::new()));
    let samples_cb = samples.clone();

    let err_fn = |err| eprintln!("[cpal] erro no stream de input: {}", err);

    // cpal expõe amostras no formato nativo do device (f32/i16/u16 são os mais
    // comuns). Normalizamos tudo para f32 no buffer para simplificar a etapa
    // seguinte (conversão para WAV i16). f32 tem faixa [-1.0, 1.0].
    let stream_config: cpal::StreamConfig = config.into();
    let stream = match sample_format {
        cpal::SampleFormat::F32 => device.build_input_stream(
            &stream_config,
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                samples_cb.lock().unwrap().extend_from_slice(data);
            },
            err_fn,
            None,
        )?,
        cpal::SampleFormat::I16 => device.build_input_stream(
            &stream_config,
            move |data: &[i16], _: &cpal::InputCallbackInfo| {
                let mut buf = samples_cb.lock().unwrap();
                buf.extend(data.iter().map(|&s| s as f32 / i16::MAX as f32));
            },
            err_fn,
            None,
        )?,
        cpal::SampleFormat::U16 => device.build_input_stream(
            &stream_config,
            move |data: &[u16], _: &cpal::InputCallbackInfo| {
                let mut buf = samples_cb.lock().unwrap();
                // u16 é unsigned centrado em u16::MAX/2; convertemos pra faixa [-1, 1].
                let half = u16::MAX as f32 / 2.0;
                buf.extend(data.iter().map(|&s| (s as f32 - half) / half));
            },
            err_fn,
            None,
        )?,
        other => return Err(anyhow!("formato de sample não suportado: {:?}", other)),
    };

    stream.play().context("falha ao iniciar stream do microfone")?;

    Ok(Recording {
        _stream: stream,
        samples,
        sample_rate,
        channels,
    })
}

/// Para o stream (implicitamente ao dropar `_stream`) e escreve o WAV.
///
/// Retorna o path do arquivo criado em `%TEMP%\whisper_recording_<timestamp>.wav`.
fn finalize_recording(recording: Recording) -> Result<PathBuf> {
    // O drop do stream vai acontecer no fim da função (via drop de `recording`).
    // Copiamos os samples antes disso para não segurar o Mutex durante a escrita.
    let samples: Vec<f32> = {
        let guard = recording
            .samples
            .lock()
            .map_err(|_| anyhow!("mutex de samples envenenado"))?;
        guard.clone()
    };

    if samples.is_empty() {
        return Err(anyhow!("nenhum áudio capturado (gravação muito curta?)"));
    }

    // Escrevemos como PCM 16-bit por dois motivos:
    //   1. Máxima compatibilidade com players/parsers de WAV.
    //   2. Whisper.cpp lê PCM 16-bit nativamente sem conversão.
    let spec = hound::WavSpec {
        channels: recording.channels,
        sample_rate: recording.sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };

    let path = temp_wav_path();
    let mut writer = hound::WavWriter::create(&path, spec)
        .with_context(|| format!("falha ao criar WAV em {:?}", path))?;

    // Converte f32 [-1.0, 1.0] pra i16 [-32768, 32767], clampando pra evitar
    // overflow em samples que estejam ligeiramente fora da faixa (pode acontecer
    // com processamento de driver).
    for f in samples {
        let clamped = f.clamp(-1.0, 1.0);
        let s = (clamped * i16::MAX as f32) as i16;
        writer.write_sample(s)?;
    }
    writer.finalize().context("falha ao finalizar WAV")?;

    Ok(path)
}

/// Lista os nomes de todos os dispositivos de entrada de áudio disponíveis.
/// Usado pela UI de settings pra montar o dropdown de escolha de microfone.
pub fn list_devices() -> Result<Vec<String>> {
    let host = cpal::default_host();
    let devices = host
        .input_devices()
        .context("falha ao listar dispositivos de entrada")?;
    Ok(devices.filter_map(|d| d.name().ok()).collect())
}

/// Path único para o WAV, baseado no timestamp em ms.
fn temp_wav_path() -> PathBuf {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("whisper_recording_{}.wav", ts))
}
