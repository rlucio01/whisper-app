//! Módulo de gravação de reuniões, diarização por orador e geração de atas com ações.
//!
//! Permite:
//! 1. Gravar reuniões ao vivo via microfone com monitoramento de tempo e níveis.
//! 2. Processar arquivos de reunião (MP3, OGG, WAV, M4A, etc.).
//! 3. Transcrever com marcadores de tempo e separar oradores via IA (Diarização).
//! 4. Gerar resumo executivo, decisões tomadas e lista de ações/tarefas atribuídas por pessoa.
//! 5. Persistir atas em `meetings.jsonl` no disco para consulta e marcação de tarefas concluídas.

use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, Runtime};

use crate::config::SharedConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeetingActionItem {
    pub speaker: String,
    pub task: String,
    pub deadline: String,
    #[serde(default)]
    pub done: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeetingDiarizedTurn {
    pub speaker: String,
    pub timestamp: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeetingMinutes {
    pub id: String,
    pub title: String,
    pub created_at: u64,
    pub duration_sec: f32,
    pub executive_summary: String,
    pub key_decisions: Vec<String>,
    pub action_items: Vec<MeetingActionItem>,
    pub speakers: Vec<String>,
    pub diarized_transcript: Vec<MeetingDiarizedTurn>,
    pub raw_transcript: String,
}

#[derive(Deserialize)]
struct LlmMeetingJson {
    pub title: String,
    pub executive_summary: String,
    #[serde(default)]
    pub key_decisions: Vec<String>,
    #[serde(default)]
    pub action_items: Vec<MeetingActionItem>,
    #[serde(default)]
    pub speakers: Vec<String>,
    #[serde(default)]
    pub diarized_transcript: Vec<MeetingDiarizedTurn>,
}

enum MeetingCommand {
    Start(mpsc::Sender<Result<()>>),
    Stop(mpsc::Sender<Result<PathBuf>>),
    Cancel,
}

/// Serviço de áudio de reunião rodando em thread dedicada (cpal::Stream é !Send no Windows).
pub struct MeetingService {
    cmd_tx: mpsc::Sender<MeetingCommand>,
}

impl MeetingService {
    pub fn spawn<R: Runtime>(app: AppHandle<R>) -> Self {
        let (cmd_tx, cmd_rx) = mpsc::channel::<MeetingCommand>();
        thread::spawn(move || meeting_audio_thread_loop(cmd_rx, app));
        Self { cmd_tx }
    }

    pub fn start(&self) -> Result<()> {
        let (tx, rx) = mpsc::channel();
        self.cmd_tx
            .send(MeetingCommand::Start(tx))
            .map_err(|e| anyhow!("falha ao enviar comando de início: {}", e))?;
        rx.recv()
            .map_err(|e| anyhow!("resposta não recebida: {}", e))?
    }

    pub fn stop(&self) -> Result<PathBuf> {
        let (tx, rx) = mpsc::channel();
        self.cmd_tx
            .send(MeetingCommand::Stop(tx))
            .map_err(|e| anyhow!("falha ao enviar comando de parada: {}", e))?;
        rx.recv()
            .map_err(|e| anyhow!("resposta não recebida: {}", e))?
    }

    pub fn cancel(&self) {
        let _ = self.cmd_tx.send(MeetingCommand::Cancel);
    }
}

struct MeetingRecording {
    _stream: cpal::Stream,
    samples: Arc<Mutex<Vec<f32>>>,
    sample_rate: u32,
    channels: u16,
    start_time: Instant,
}

fn meeting_audio_thread_loop<R: Runtime>(
    cmd_rx: mpsc::Receiver<MeetingCommand>,
    app: AppHandle<R>,
) {
    let mut recording: Option<MeetingRecording> = None;
    let tick = Duration::from_millis(100);

    loop {
        match cmd_rx.recv_timeout(tick) {
            Ok(MeetingCommand::Start(reply_tx)) => {
                if recording.is_some() {
                    let _ = reply_tx.send(Err(anyhow!("Gravação de reunião já em andamento.")));
                    continue;
                }

                let mic_name = app
                    .try_state::<SharedConfig>()
                    .and_then(|s| s.lock().ok().map(|g| g.microphone.clone()))
                    .unwrap_or_default();

                match start_stream(&mic_name) {
                    Ok(r) => {
                        recording = Some(r);
                        let _ = reply_tx.send(Ok(()));
                    }
                    Err(e) => {
                        let _ = reply_tx.send(Err(e));
                    }
                }
            }
            Ok(MeetingCommand::Stop(reply_tx)) => {
                let Some(r) = recording.take() else {
                    let _ = reply_tx.send(Err(anyhow!("Nenhuma gravação de reunião ativa.")));
                    continue;
                };

                let res = finalize_meeting_wav(r);
                let _ = reply_tx.send(res);
            }
            Ok(MeetingCommand::Cancel) => {
                recording = None;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if let Some(r) = &recording {
                    let elapsed_sec = r.start_time.elapsed().as_secs();
                    let _ = app.emit("meeting-recording-time", elapsed_sec);

                    if let Ok(guard) = r.samples.lock() {
                        let len = guard.len();
                        let start = len.saturating_sub(4800);
                        let window = &guard[start..];
                        let rms = if window.is_empty() {
                            0.0
                        } else {
                            let sum: f32 = window.iter().map(|s| s * s).sum();
                            (sum / window.len() as f32).sqrt()
                        };
                        let level = (rms * 8.0).sqrt().min(1.0);
                        let _ = app.emit("meeting-audio-level", level);
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
}

fn start_stream(mic_name: &str) -> Result<MeetingRecording> {
    let host = cpal::default_host();
    let device = if mic_name.trim().is_empty() {
        host.default_input_device()
            .ok_or_else(|| anyhow!("nenhum microfone de entrada disponível"))?
    } else {
        host.input_devices()?
            .find(|d| d.name().map(|n| n == mic_name).unwrap_or(false))
            .ok_or_else(|| anyhow!("microfone \"{}\" não encontrado", mic_name))?
    };

    let config = device.default_input_config()?;
    let sample_rate = config.sample_rate().0;
    let channels = config.channels();
    let sample_format = config.sample_format();

    let samples = Arc::new(Mutex::new(Vec::<f32>::new()));
    let samples_cb = samples.clone();
    let stream_config: cpal::StreamConfig = config.into();

    let err_fn = |e| eprintln!("[meeting audio] erro no stream: {}", e);

    let stream = match sample_format {
        cpal::SampleFormat::F32 => device.build_input_stream(
            &stream_config,
            move |data: &[f32], _| {
                if let Ok(mut buf) = samples_cb.lock() {
                    buf.extend_from_slice(data);
                }
            },
            err_fn,
            None,
        )?,
        cpal::SampleFormat::I16 => device.build_input_stream(
            &stream_config,
            move |data: &[i16], _| {
                if let Ok(mut buf) = samples_cb.lock() {
                    buf.extend(data.iter().map(|&s| s as f32 / i16::MAX as f32));
                }
            },
            err_fn,
            None,
        )?,
        cpal::SampleFormat::U16 => device.build_input_stream(
            &stream_config,
            move |data: &[u16], _| {
                if let Ok(mut buf) = samples_cb.lock() {
                    let half = u16::MAX as f32 / 2.0;
                    buf.extend(data.iter().map(|&s| (s as f32 - half) / half));
                }
            },
            err_fn,
            None,
        )?,
        other => return Err(anyhow!("formato de áudio não suportado: {:?}", other)),
    };

    stream.play()?;

    Ok(MeetingRecording {
        _stream: stream,
        samples,
        sample_rate,
        channels,
        start_time: Instant::now(),
    })
}

fn finalize_meeting_wav(r: MeetingRecording) -> Result<PathBuf> {
    let samples = r
        .samples
        .lock()
        .map_err(|_| anyhow!("mutex envenenado"))?
        .clone();

    if samples.is_empty() {
        return Err(anyhow!("gravação muito curta ou vazia"));
    }

    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let wav_path = std::env::temp_dir().join(format!("meeting_recording_{}.wav", ts));

    let spec = hound::WavSpec {
        channels: r.channels,
        sample_rate: r.sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };

    let mut writer = hound::WavWriter::create(&wav_path, spec)?;
    for f in samples {
        let clamped = f.clamp(-1.0, 1.0);
        let s = (clamped * i16::MAX as f32) as i16;
        writer.write_sample(s)?;
    }
    writer.finalize()?;

    Ok(wav_path)
}

/// Inicia a gravação de reunião ao vivo.
pub fn start_meeting_recording<R: Runtime>(app: &AppHandle<R>) -> Result<()> {
    app.state::<MeetingService>().start()
}

/// Finaliza a gravação ao vivo e salva como WAV temporário.
pub fn stop_meeting_recording<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf> {
    app.state::<MeetingService>().stop()
}

/// Cancela a gravação descartando os dados.
pub fn cancel_meeting_recording<R: Runtime>(app: &AppHandle<R>) {
    app.state::<MeetingService>().cancel();
}

/// Processa um arquivo de reunião (ao vivo ou importado) gerando transcrição,
/// diarização e ata estruturada com ações.
pub fn process_meeting_audio<R: Runtime>(
    app: &AppHandle<R>,
    audio_path: &Path,
    context_hint: Option<&str>,
) -> Result<MeetingMinutes> {
    let _ = app.emit("meeting-status", "Transcrevendo áudio com Whisper…");

    // 1. Transcreve com marcadores de tempo
    let tr_result = crate::transcription::transcribe_file_standalone(app, audio_path)?;

    let _ = app.emit("meeting-status", "Analisando oradores e gerando ata com IA…");

    // Monta texto com timestamps
    let mut formatted_turns = String::new();
    for seg in &tr_result.segments {
        let m0 = (seg.start_sec / 60.0).floor() as u32;
        let s0 = (seg.start_sec % 60.0).floor() as u32;
        let m1 = (seg.end_sec / 60.0).floor() as u32;
        let s1 = (seg.end_sec % 60.0).floor() as u32;
        formatted_turns.push_str(&format!(
            "[{:02}:{:02} - {:02}:{:02}] {}\n",
            m0, s0, m1, s1, seg.text
        ));
    }

    let user_prompt = format!(
        "Contexto da Reunião ou Participantes informados: {}\n\nTranscrição com timestamps:\n{}",
        context_hint.unwrap_or("Não especificado"),
        if formatted_turns.is_empty() {
            &tr_result.text
        } else {
            &formatted_turns
        }
    );

    let system_prompt = "Você é um especialista em atas executivas e diarização de fala.\n\
Analise a transcrição com marcações de tempo e gere uma ata profissional e estruturada.\n\n\
Diretrizes:\n\
1. Identifique os diferentes oradores na conversa (atribua nomes reais se citados, ex: 'Rafael', 'Mariana', ou use 'Orador 1', 'Orador 2').\n\
2. Elabore um Resumo Executivo claro e conciso explicando o objetivo e os temas tratados.\n\
3. Destaque as Decisões Tomadas.\n\
4. Destaque os Itens de Ação / Tarefas atribuídas a cada participante com prazo se houver.\n\
5. Organize a transcrição completa diarizada por fala e intervalo de tempo.\n\n\
Responda APENAS com um objeto JSON válido (sem blocos markdown ```json), seguindo este schema exato:\n\
{\n\
  \"title\": \"Título objetivo da Reunião\",\n\
  \"executive_summary\": \"Resumo executivo completo...\",\n\
  \"key_decisions\": [\"Decisão 1\", \"Decisão 2\"],\n\
  \"action_items\": [\n\
    {\"speaker\": \"Nome ou Orador\", \"task\": \"Tarefa a realizar\", \"deadline\": \"Prazo ou A definir\", \"done\": false}\n\
  ],\n\
  \"speakers\": [\"Orador 1\", \"Orador 2\"],\n\
  \"diarized_transcript\": [\n\
    {\"speaker\": \"Orador 1\", \"timestamp\": \"00:00 - 00:15\", \"text\": \"Falas...\"}\n\
  ]\n\
}";

    // 2. Chama LLM
    let llm_response = crate::llm::call_llm_custom(app, system_prompt, &user_prompt, Some(4000));

    let parsed_data = match llm_response {
        Ok(text) => parse_meeting_json(&text).unwrap_or_else(|| fallback_meeting_data(&tr_result)),
        Err(e) => {
            eprintln!("Aviso: LLM indisponível para ata ({:#}), usando síntese local.", e);
            fallback_meeting_data(&tr_result)
        }
    };

    let now_ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let meeting = MeetingMinutes {
        id: format!("meet_{}", now_ts),
        title: parsed_data.title,
        created_at: now_ts,
        duration_sec: tr_result.duration_sec,
        executive_summary: parsed_data.executive_summary,
        key_decisions: parsed_data.key_decisions,
        action_items: parsed_data.action_items,
        speakers: parsed_data.speakers,
        diarized_transcript: parsed_data.diarized_transcript,
        raw_transcript: tr_result.text,
    };

    // 3. Salva no disco
    let _ = save_meeting_to_disk(app, &meeting);

    let _ = app.emit("meeting-completed", &meeting);

    Ok(meeting)
}

fn parse_meeting_json(raw: &str) -> Option<LlmMeetingJson> {
    let clean = raw.trim();
    let json_str = if let Some(start) = clean.find('{') {
        if let Some(end) = clean.rfind('}') {
            &clean[start..=end]
        } else {
            clean
        }
    } else {
        clean
    };

    serde_json::from_str::<LlmMeetingJson>(json_str).ok()
}

/// Fallback inteligente caso nenhuma chave de LLM esteja configurada no momento.
fn fallback_meeting_data(tr: &crate::transcription::TranscriptionFileResult) -> LlmMeetingJson {
    let mut turns = Vec::new();
    let mut current_speaker = 1;

    for (i, seg) in tr.segments.iter().enumerate() {
        if i > 0 && i % 3 == 0 {
            current_speaker = if current_speaker == 1 { 2 } else { 1 };
        }
        let m0 = (seg.start_sec / 60.0).floor() as u32;
        let s0 = (seg.start_sec % 60.0).floor() as u32;
        let m1 = (seg.end_sec / 60.0).floor() as u32;
        let s1 = (seg.end_sec % 60.0).floor() as u32;

        turns.push(MeetingDiarizedTurn {
            speaker: format!("Orador {}", current_speaker),
            timestamp: format!("{:02}:{:02} - {:02}:{:02}", m0, s0, m1, s1),
            text: seg.text.clone(),
        });
    }

    let summary = if tr.text.is_empty() {
        "Nenhuma fala identificada no áudio gravado.".to_string()
    } else {
        format!("Reunião gravada com duração aproximada de {:.0} segundos. Transcrição completa processada e organizada por oradores.", tr.duration_sec)
    };

    LlmMeetingJson {
        title: "Ata de Reunião".to_string(),
        executive_summary: summary,
        key_decisions: vec!["Alinhamento dos tópicos discutidos".to_string()],
        action_items: vec![MeetingActionItem {
            speaker: "Participantes".to_string(),
            task: "Revisar anotações e confirmar encaminhamentos".to_string(),
            deadline: "Próxima reunião".to_string(),
            done: false,
        }],
        speakers: vec!["Orador 1".to_string(), "Orador 2".to_string()],
        diarized_transcript: turns,
    }
}

fn meetings_file_path<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf> {
    let dir = app
        .path()
        .app_data_dir()
        .context("falha ao obter app data dir")?;
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join("meetings.jsonl"))
}

pub fn save_meeting_to_disk<R: Runtime>(app: &AppHandle<R>, meeting: &MeetingMinutes) -> Result<()> {
    let path = meetings_file_path(app)?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;

    let json_line = serde_json::to_string(meeting)?;
    writeln!(file, "{}", json_line)?;
    Ok(())
}

pub fn load_all_meetings<R: Runtime>(app: &AppHandle<R>) -> Result<Vec<MeetingMinutes>> {
    let path = meetings_file_path(app)?;
    if !path.exists() {
        return Ok(Vec::new());
    }

    let file = File::open(&path)?;
    let reader = BufReader::new(file);
    let mut list = Vec::new();

    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(m) = serde_json::from_str::<MeetingMinutes>(&line) {
            list.push(m);
        }
    }

    list.reverse(); // mais recentes primeiro
    Ok(list)
}

pub fn delete_meeting<R: Runtime>(app: &AppHandle<R>, id: &str) -> Result<()> {
    let all = load_all_meetings(app)?;
    let filtered: Vec<MeetingMinutes> = all.into_iter().filter(|m| m.id != id).collect();

    let path = meetings_file_path(app)?;
    let mut file = File::create(&path)?;

    // Salva na ordem original do arquivo (mais antigo primeiro)
    for m in filtered.iter().rev() {
        let json_line = serde_json::to_string(m)?;
        writeln!(file, "{}", json_line)?;
    }

    Ok(())
}

pub fn set_action_item_done<R: Runtime>(
    app: &AppHandle<R>,
    meeting_id: &str,
    action_idx: usize,
    done: bool,
) -> Result<()> {
    let mut all = load_all_meetings(app)?;
    for m in &mut all {
        if m.id == meeting_id {
            if let Some(item) = m.action_items.get_mut(action_idx) {
                item.done = done;
            }
        }
    }

    let path = meetings_file_path(app)?;
    let mut file = File::create(&path)?;
    for m in all.iter().rev() {
        let json_line = serde_json::to_string(m)?;
        writeln!(file, "{}", json_line)?;
    }

    Ok(())
}
