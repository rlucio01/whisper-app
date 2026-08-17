//! Transcrição de áudio.
//!
//! Suporta três modos (escolhidos pelo config `transcription_provider`):
//!   - **Local** — whisper.cpp via `whisper-rs`, offline. Usa o modelo
//!     escolhido em `config.whisper_model` (tiny/base/small/medium/large-turbo);
//!     o arquivo é baixado pela UI de settings.
//!   - **OpenAI Cloud** — API `POST /v1/audio/transcriptions` com o modelo
//!     `whisper-1`. Envia o WAV como multipart/form-data. Depende da
//!     `openai_api_key` do config.
//!   - **Groq Cloud** — mesmo formato de request (a API do Groq é compatível
//!     com a da OpenAI), mas roda `whisper-large-v3-turbo` em LPU — bem mais
//!     rápido pra áudios curtos. Depende da `groq_api_key` do config.
//!
//! ## Thread dedicada
//!
//! Uma thread possui o `WhisperContext` (para modo local) e recebe caminhos
//! de WAV via `mpsc::channel`. O modelo é recarregado lazy: só na primeira
//! transcrição local, ou quando o usuário troca o modelo em settings.
//!
//! ## Pipeline de áudio (só modo local)
//!
//! Whisper.cpp exige 16kHz mono f32. O WAV que o [`crate::audio`] gera pode
//! estar em qualquer sample rate/channels, então convertemos aqui.
//! No modo cloud, mandamos o WAV como está — a API aceita qualquer sample rate.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use tauri::{AppHandle, Emitter, Manager, Runtime};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::config::{SharedConfig, TranscriptionProvider, WhisperModel};
use crate::llm::LlmService;
use crate::models;
use crate::visual;

/// Sample rate exigido pelo whisper.cpp local.
const WHISPER_SAMPLE_RATE: u32 = 16_000;

/// Endpoint da OpenAI Whisper API. Modelo `whisper-1` é o único público.
const OPENAI_ENDPOINT: &str = "https://api.openai.com/v1/audio/transcriptions";
const OPENAI_MODEL: &str = "whisper-1";

/// Endpoint do Groq — compatível com o formato multipart da OpenAI.
/// `whisper-large-v3-turbo` é o melhor custo/latência do catálogo deles
/// pra transcrição (o `whisper-large-v3` normal é mais preciso mas mais lento).
const GROQ_ENDPOINT: &str = "https://api.groq.com/openai/v1/audio/transcriptions";
const GROQ_MODEL: &str = "whisper-large-v3-turbo";

/// Timeout total pra chamada cloud. Áudios longos podem levar dezenas de
/// segundos; 5min é folgado o suficiente pra qualquer ditado normal.
const CLOUD_REQUEST_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// Comandos aceitos pela thread de transcrição.
enum Command {
    /// Pede pra carregar o modelo local (do config atual) sem transcrever nada.
    /// Usado no boot pra eliminar o custo da 1ª transcrição — o load leva
    /// ~1-2s e sem warmup isso vira latência visível pro usuário.
    Warmup,
    /// Transcreve o WAV apontado por esse path.
    Transcribe(PathBuf),
}

/// Handle público para pedir transcrições.
pub struct TranscriptionService {
    cmd_tx: mpsc::Sender<Command>,
}

impl TranscriptionService {
    pub fn spawn<R: Runtime>(app: AppHandle<R>) -> Self {
        let (cmd_tx, cmd_rx) = mpsc::channel::<Command>();

        // HTTP client reutilizado para chamadas cloud — mantém keep-alive
        // TLS quente entre requests. Timeout generoso pra áudios longos.
        let client = reqwest::blocking::Client::builder()
            .timeout(CLOUD_REQUEST_TIMEOUT)
            .pool_max_idle_per_host(2)
            .build()
            .expect("falha ao criar HTTP client de transcrição");

        thread::spawn(move || transcription_thread_loop(cmd_rx, app, client));

        // Dispara o warmup imediatamente — vai carregar o modelo em paralelo
        // com o resto da inicialização do app. Se o provider for cloud, o
        // loop simplesmente ignora (não faz nada).
        let _ = cmd_tx.send(Command::Warmup);

        Self { cmd_tx }
    }

    /// Enfileira uma transcrição. Não bloqueia.
    pub fn transcribe(&self, wav_path: PathBuf) {
        let _ = self.cmd_tx.send(Command::Transcribe(wav_path));
    }
}

/// Contexto local carregado + qual modelo ele representa (pra saber se
/// precisamos recarregar quando o usuário troca em settings).
struct LoadedModel {
    which: WhisperModel,
    ctx: WhisperContext,
}

/// Loop principal da thread de transcrição.
/// Mantém o modelo local carregado entre chamadas — recarrega se o usuário
/// mudar o modelo em settings.
fn transcription_thread_loop<R: Runtime>(
    cmd_rx: mpsc::Receiver<Command>,
    app: AppHandle<R>,
    client: reqwest::blocking::Client,
) {
    let mut loaded: Option<LoadedModel> = None;

    while let Ok(cmd) = cmd_rx.recv() {
        // Snapshot do config a cada mensagem — a UI pode ter mudado provider
        // ou modelo entre gravações. Também aproveita a chave da OpenAI.
        let cfg_snapshot = match app.state::<SharedConfig>().lock() {
            Ok(g) => g.clone(),
            Err(_) => {
                if let Command::Transcribe(wav_path) = cmd {
                    let _ = app.emit(
                        "transcription-error",
                        "config mutex envenenado".to_string(),
                    );
                    visual::set(&app, visual::State::Idle);
                    let _ = std::fs::remove_file(&wav_path);
                }
                continue;
            }
        };

        match cmd {
            Command::Warmup => {
                // Só faz sentido pré-carregar se o provider for local — cloud
                // não tem estado local para aquecer.
                if cfg_snapshot.transcription_provider == TranscriptionProvider::Local {
                    if let Err(e) = ensure_model_loaded(
                        &app,
                        cfg_snapshot.whisper_model,
                        &mut loaded,
                        /*emit_status=*/ false,
                    ) {
                        // Warmup falhou (ex: modelo não baixado) — não é
                        // fatal, o usuário vai receber o erro real na 1ª
                        // gravação. Só logamos.
                        eprintln!("[warmup] {:#}", e);
                    }
                }
            }
            Command::Transcribe(wav_path) => {
                let result = match cfg_snapshot.transcription_provider {
                    TranscriptionProvider::Local => transcribe_local(
                        &app,
                        &wav_path,
                        cfg_snapshot.whisper_model,
                        &mut loaded,
                    ),
                    TranscriptionProvider::OpenaiCloud => {
                        let _ = app.emit("transcription-status", "enviando_para_cloud");
                        transcribe_cloud(
                            &client,
                            &wav_path,
                            CloudTranscribeConfig {
                                endpoint: OPENAI_ENDPOINT,
                                model: OPENAI_MODEL,
                                api_key: &cfg_snapshot.openai_api_key,
                                label: "OpenAI",
                            },
                        )
                    }
                    TranscriptionProvider::GroqCloud => {
                        let _ = app.emit("transcription-status", "enviando_para_cloud");
                        transcribe_cloud(
                            &client,
                            &wav_path,
                            CloudTranscribeConfig {
                                endpoint: GROQ_ENDPOINT,
                                model: GROQ_MODEL,
                                api_key: &cfg_snapshot.groq_api_key,
                                label: "Groq",
                            },
                        )
                    }
                };

                match result {
                    Ok(text) => {
                        let _ = app.emit("transcription-complete", text.clone());
                        app.state::<LlmService>().format(text);
                    }
                    Err(e) => {
                        let _ = app.emit("transcription-error", format!("{:#}", e));
                        visual::set(&app, visual::State::Idle);
                    }
                }

                let _ = std::fs::remove_file(&wav_path);
            }
        }
    }
}

/// Garante que `loaded` contém o modelo `wanted` carregado. Se ainda não
/// existe ou é outro modelo, recarrega. `emit_status=true` emite o evento
/// `transcription-status = carregando_modelo` (usado quando a UI espera
/// feedback — no warmup, silencioso).
fn ensure_model_loaded<R: Runtime>(
    app: &AppHandle<R>,
    wanted: WhisperModel,
    loaded: &mut Option<LoadedModel>,
    emit_status: bool,
) -> Result<()> {
    let needs_reload = match loaded {
        Some(l) => l.which != wanted,
        None => true,
    };
    if !needs_reload {
        return Ok(());
    }
    if emit_status {
        let _ = app.emit("transcription-status", "carregando_modelo");
    }
    let ctx = load_local_model(app, wanted)?;
    *loaded = Some(LoadedModel { which: wanted, ctx });
    Ok(())
}

// ---------- Modo local ----------

/// Transcreve usando o modelo local, recarregando-o se o usuário mudou de
/// modelo desde a última transcrição.
fn transcribe_local<R: Runtime>(
    app: &AppHandle<R>,
    wav_path: &Path,
    wanted: WhisperModel,
    loaded: &mut Option<LoadedModel>,
) -> Result<String> {
    ensure_model_loaded(app, wanted, loaded, /*emit_status=*/ true)?;

    let ctx = &loaded
        .as_ref()
        .expect("modelo foi carregado logo acima")
        .ctx;

    let _ = app.emit("transcription-status", "transcrevendo");
    transcribe_wav_local(ctx, wav_path)
}

/// Carrega o modelo local do disco. Falha com mensagem amigável se o arquivo
/// não existe — o usuário precisa ir em settings e baixar.
fn load_local_model<R: Runtime>(app: &AppHandle<R>, m: WhisperModel) -> Result<WhisperContext> {
    let path = models::file_path(app, m)?;
    let meta = m.meta();

    if !path.exists() {
        return Err(anyhow!(
            "Modelo \"{}\" ainda não foi baixado.\n\
             Vá em Configurações → Transcrição → Modelo Whisper e clique em Baixar.\n\
             (arquivo esperado: {})",
            meta.display_name,
            path.display()
        ));
    }

    let path_str = path
        .to_str()
        .ok_or_else(|| anyhow!("path do modelo tem caracteres inválidos"))?;

    WhisperContext::new_with_params(path_str, WhisperContextParameters::default())
        .with_context(|| format!("falha ao carregar modelo em {}", path.display()))
}

/// Executa a transcrição local usando o modelo já carregado.
fn transcribe_wav_local(ctx: &WhisperContext, wav_path: &Path) -> Result<String> {
    let audio = read_and_prepare_wav(wav_path)?;

    let mut state = ctx
        .create_state()
        .context("falha ao criar state do whisper")?;

    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    params.set_language(None);
    params.set_print_progress(false);
    params.set_print_special(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    // whisper.cpp por padrão usa só min(4, núcleos) threads — em CPUs com
    // mais de 4 núcleos isso deixa a maior parte ociosa. Usamos todos os
    // núcleos lógicos disponíveis.
    let n_threads = std::thread::available_parallelism()
        .map(|n| n.get() as i32)
        .unwrap_or(4);
    params.set_n_threads(n_threads);

    state
        .full(params, &audio)
        .context("falha durante a transcrição")?;

    let num_segments = state.full_n_segments();
    let mut text = String::new();
    for i in 0..num_segments {
        let segment = state
            .get_segment(i)
            .ok_or_else(|| anyhow!("segmento {} não encontrado", i))?;
        let piece = segment
            .to_str()
            .with_context(|| format!("falha ao ler segmento {}", i))?;
        text.push_str(piece);
    }

    Ok(text.trim().to_string())
}

// ---------- Modo cloud (OpenAI ou Groq Whisper API — mesmo formato) ----------

/// Endpoint, modelo e chave de um provider de transcrição cloud. OpenAI e
/// Groq compartilham o mesmo formato de request/response (multipart com
/// `model` + `file`, resposta `{ text }`), então uma função só atende os dois.
struct CloudTranscribeConfig<'a> {
    endpoint: &'static str,
    model: &'static str,
    api_key: &'a str,
    /// Nome do provider — usado em mensagens de erro.
    label: &'static str,
}

fn transcribe_cloud(
    client: &reqwest::blocking::Client,
    wav_path: &Path,
    cfg: CloudTranscribeConfig<'_>,
) -> Result<String> {
    if cfg.api_key.trim().is_empty() {
        return Err(anyhow!(
            "Nenhuma chave da {} configurada.\n\
             Vá em Configurações e cole sua chave — ou troque a transcrição\n\
             de volta para \"Local\".",
            cfg.label
        ));
    }

    // Downsample pra mono 16kHz antes de enviar. O provider internamente já
    // converte, então mandar o original em 48kHz stereo só desperdiça upload.
    // Reduzir aqui corta o payload em ~5-10x (44.1kHz stereo → 16kHz mono),
    // o que em conexões medianas economiza alguns segundos por transcrição.
    let (send_path, _cleanup) = prepare_cloud_upload(wav_path)?;

    // `Form::file` lê o arquivo por streaming, sem carregar tudo em RAM.
    // A extensão `.wav` no filename orienta o parser do provider.
    let form = reqwest::blocking::multipart::Form::new()
        .text("model", cfg.model)
        .text("response_format", "json")
        .file("file", &send_path)
        .with_context(|| format!("falha ao anexar {}", send_path.display()))?;

    let response = client
        .post(cfg.endpoint)
        .bearer_auth(cfg.api_key)
        .multipart(form)
        .send()
        .with_context(|| format!("falha ao enviar áudio para a API {}", cfg.label))?;

    let status = response.status();
    if !status.is_success() {
        let body = response
            .text()
            .unwrap_or_else(|_| "(sem corpo de resposta)".to_string());
        return Err(anyhow!(
            "{} Whisper API retornou {}: {}",
            cfg.label,
            status,
            body
        ));
    }

    let parsed: CloudResponse = response
        .json()
        .with_context(|| format!("resposta da {} Whisper não é JSON válido", cfg.label))?;

    if parsed.text.trim().is_empty() {
        return Err(anyhow!("{} Whisper retornou texto vazio", cfg.label));
    }

    Ok(parsed.text.trim().to_string())
}

#[derive(serde::Deserialize)]
struct CloudResponse {
    text: String,
}

// ---------- Preparação do upload (só modo cloud) ----------

/// Guard RAII: apaga o WAV temporário no drop se `Some`. `None` quando o
/// arquivo original já tá no formato ideal e não geramos um temp.
struct TempWavCleanup(Option<PathBuf>);
impl Drop for TempWavCleanup {
    fn drop(&mut self) {
        if let Some(p) = self.0.take() {
            let _ = std::fs::remove_file(p);
        }
    }
}

/// Se necessário, cria um WAV downsampled (mono 16kHz) num arquivo temp e
/// retorna esse path. Se o original já é mono 16kHz, retorna o próprio path
/// original (o `TempWavCleanup` fica vazio, nada é apagado).
fn prepare_cloud_upload(original: &Path) -> Result<(PathBuf, TempWavCleanup)> {
    // Peek na spec sem carregar samples ainda — se já for 16kHz mono, atalho.
    let spec = hound::WavReader::open(original)
        .with_context(|| format!("falha ao abrir WAV em {}", original.display()))?
        .spec();

    if spec.sample_rate == WHISPER_SAMPLE_RATE && spec.channels == 1 {
        return Ok((original.to_path_buf(), TempWavCleanup(None)));
    }

    // Caso contrário, converte via a mesma pipeline que usamos pro modo local
    // (mono + resample linear 16kHz) e escreve como PCM 16-bit num temp.
    let samples = read_and_prepare_wav(original)?;

    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let temp = std::env::temp_dir().join(format!("whisper_upload_{}.wav", ts));

    let spec_out = hound::WavSpec {
        channels: 1,
        sample_rate: WHISPER_SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(&temp, spec_out)
        .with_context(|| format!("falha ao criar {} para upload", temp.display()))?;
    for f in samples {
        let clamped = f.clamp(-1.0, 1.0);
        let s = (clamped * i16::MAX as f32) as i16;
        writer.write_sample(s)?;
    }
    writer.finalize().context("falha ao finalizar WAV do upload")?;

    Ok((temp.clone(), TempWavCleanup(Some(temp))))
}

// ---------- Preparação do WAV (só modo local) ----------

/// Lê o WAV, converte pra mono e resamplea pra 16kHz.
fn read_and_prepare_wav(path: &Path) -> Result<Vec<f32>> {
    let mut reader = hound::WavReader::open(path)
        .with_context(|| format!("falha ao abrir WAV em {}", path.display()))?;
    let spec = reader.spec();

    let raw: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Int => match spec.bits_per_sample {
            16 => reader
                .samples::<i16>()
                .map(|s| s.map(|s| s as f32 / i16::MAX as f32))
                .collect::<std::result::Result<Vec<_>, _>>()?,
            other => return Err(anyhow!("WAV int {}bit não suportado", other)),
        },
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .collect::<std::result::Result<Vec<_>, _>>()?,
    };

    let mono = downmix_to_mono(&raw, spec.channels);

    let resampled = if spec.sample_rate == WHISPER_SAMPLE_RATE {
        mono
    } else {
        linear_resample(&mono, spec.sample_rate, WHISPER_SAMPLE_RATE)
    };

    Ok(resampled)
}

fn downmix_to_mono(samples: &[f32], channels: u16) -> Vec<f32> {
    if channels == 1 {
        return samples.to_vec();
    }
    let ch = channels as usize;
    samples
        .chunks_exact(ch)
        .map(|frame| frame.iter().sum::<f32>() / ch as f32)
        .collect()
}

fn linear_resample(samples: &[f32], src_rate: u32, dst_rate: u32) -> Vec<f32> {
    if samples.is_empty() {
        return Vec::new();
    }
    let ratio = src_rate as f64 / dst_rate as f64;
    let out_len = (samples.len() as f64 / ratio).round() as usize;

    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let src_pos = i as f64 * ratio;
        let src_idx = src_pos as usize;
        let frac = (src_pos - src_idx as f64) as f32;

        let s0 = samples[src_idx.min(samples.len() - 1)];
        let s1 = samples.get(src_idx + 1).copied().unwrap_or(s0);

        out.push(s0 + (s1 - s0) * frac);
    }
    out
}
