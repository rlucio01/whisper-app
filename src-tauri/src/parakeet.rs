//! Módulo de gerenciamento e execução do NVIDIA Parakeet-TDT 0.6B v3.
//!
//! O Parakeet-TDT (Token-and-Duration Transducer) da NVIDIA é um modelo de ponta
//! para transcrição e diarização (separação de oradores) local offline.
//!
//! Ele suporta 25 idiomas europeus (incluindo Português `pt` e `pt-BR`), possui
//! marcadores de troca de orador (`<|spkchange|>`) e produz alinhamento temporal
//! exato por palavra/token.
//!
//! ## Estrutura em Disco:
//! - Runtime sherpa-onnx: `%APPDATA%\com.rlucio.whisperapp\runtimes\sherpa\`
//! - Pesos do Modelo: `%APPDATA%\com.rlucio.whisperapp\models\parakeet-tdt-0.6b-v3\`
//!     - tokens.txt (~94 KB)
//!     - decoder.int8.onnx (~11.8 MB)
//!     - joiner.int8.onnx (~6.35 MB)
//!     - encoder.int8.onnx (~652 MB)
//! Total: ~670 MB

use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, Runtime};

use crate::transcription::{TranscriptSegment, TranscriptionFileResult};

/// Flag atômico para evitar disparos simultâneos de download do modelo Parakeet.
static IS_DOWNLOADING: AtomicBool = AtomicBool::new(false);

/// URLs oficiais do runtime sherpa-onnx v2.0.1 (com suporte nativo a NeMo TDT)
#[cfg(target_os = "windows")]
const SHERPA_RUNTIME_URL: &str =
    "https://github.com/k2-fsa/sherpa-onnx/releases/download/v2.0.1/sherpa-onnx-v2.0.1-win-x64-shared-MD-Release.tar.bz2";

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
const SHERPA_RUNTIME_URL: &str =
    "https://github.com/k2-fsa/sherpa-onnx/releases/download/v2.0.1/sherpa-onnx-v2.0.1-osx-arm64-shared.tar.bz2";

#[cfg(all(target_os = "macos", not(target_arch = "aarch64")))]
const SHERPA_RUNTIME_URL: &str =
    "https://github.com/k2-fsa/sherpa-onnx/releases/download/v2.0.1/sherpa-onnx-v2.0.1-osx-x64-shared.tar.bz2";

#[cfg(target_os = "linux")]
const SHERPA_RUNTIME_URL: &str =
    "https://github.com/k2-fsa/sherpa-onnx/releases/download/v2.0.1/sherpa-onnx-v2.0.1-linux-x64-shared.tar.bz2";

/// Metadados dos 4 arquivos necessários do Parakeet-TDT INT8.
struct ModelFileInfo {
    name: &'static str,
    hf_path: &'static str,
    approx_size_bytes: u64,
}

const MODEL_FILES: &[ModelFileInfo] = &[
    ModelFileInfo {
        name: "tokens.txt",
        hf_path: "tokens.txt",
        approx_size_bytes: 93_939,
    },
    ModelFileInfo {
        name: "decoder.int8.onnx",
        hf_path: "decoder.int8.onnx",
        approx_size_bytes: 11_845_275,
    },
    ModelFileInfo {
        name: "joiner.int8.onnx",
        hf_path: "joiner.int8.onnx",
        approx_size_bytes: 6_355_277,
    },
    ModelFileInfo {
        name: "encoder.int8.onnx",
        hf_path: "encoder.int8.onnx",
        approx_size_bytes: 652_184_281,
    },
];

const HF_BASE_URL: &str =
    "https://huggingface.co/csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8/resolve/main/";
const HF_MIRROR_BASE_URL: &str =
    "https://hf-mirror.com/csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8/resolve/main/";

/// Diretório do runtime sherpa-onnx.
pub fn runtime_dir<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf> {
    let app_dir = app
        .path()
        .app_data_dir()
        .context("falha ao obter app data dir")?;
    let path = app_dir.join("runtimes").join("sherpa");
    std::fs::create_dir_all(&path)
        .with_context(|| format!("falha ao criar pasta do runtime sherpa em {}", path.display()))?;
    Ok(path)
}

/// Diretório dos arquivos do modelo Parakeet-TDT.
pub fn model_dir<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf> {
    let app_dir = app
        .path()
        .app_data_dir()
        .context("falha ao obter app data dir")?;
    let path = app_dir.join("models").join("parakeet-tdt-0.6b-v3");
    std::fs::create_dir_all(&path)
        .with_context(|| format!("falha ao criar pasta do modelo parakeet em {}", path.display()))?;
    Ok(path)
}

/// Localiza o executável `sherpa-onnx-offline` (ou `.exe` no Windows).
pub fn get_sherpa_cli_path<R: Runtime>(app: &AppHandle<R>) -> Option<PathBuf> {
    let Ok(dir) = runtime_dir(app) else {
        return None;
    };

    #[cfg(target_os = "windows")]
    let exe_name = "sherpa-onnx-offline.exe";
    #[cfg(not(target_os = "windows"))]
    let exe_name = "sherpa-onnx-offline";

    let direct = dir.join(exe_name);
    if direct.exists() {
        return Some(direct);
    }

    let in_bin = dir.join("bin").join(exe_name);
    if in_bin.exists() {
        return Some(in_bin);
    }

    // Busca recursiva dentro da pasta de extração do release (ex: sherpa-onnx-v2.0.1-.../bin/...)
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let candidate1 = path.join(exe_name);
                if candidate1.exists() {
                    return Some(candidate1);
                }
                let candidate2 = path.join("bin").join(exe_name);
                if candidate2.exists() {
                    return Some(candidate2);
                }
            }
        }
    }

    None
}

/// Verifica se o runtime sherpa-onnx está instalado.
pub fn is_runtime_installed<R: Runtime>(app: &AppHandle<R>) -> bool {
    let Some(cli) = get_sherpa_cli_path(app) else {
        return false;
    };
    cli.exists()
}

/// Verifica se os arquivos do modelo Parakeet-TDT estão presentes e íntegros.
pub fn is_model_installed<R: Runtime>(app: &AppHandle<R>) -> bool {
    let Ok(dir) = model_dir(app) else {
        return false;
    };

    for item in MODEL_FILES {
        let p = dir.join(item.name);
        if !p.exists() {
            return false;
        }
        if let Ok(meta) = p.metadata() {
            // Verifica se o arquivo tem tamanho condizente (encoder deve ter pelo menos 500MB)
            if item.name.starts_with("encoder") && meta.len() < 500_000_000 {
                return false;
            }
            if meta.len() == 0 {
                return false;
            }
        } else {
            return false;
        }
    }

    true
}

/// Status do Parakeet para o frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParakeetStatus {
    pub installed: bool,
    pub is_downloading: bool,
    pub size_mb: u64,
    pub runtime_installed: bool,
    pub model_installed: bool,
    pub model_dir: Option<String>,
}

pub fn get_status<R: Runtime>(app: &AppHandle<R>) -> ParakeetStatus {
    let runtime_ok = is_runtime_installed(app);
    let model_ok = is_model_installed(app);
    let installed = runtime_ok && model_ok;

    let mut total_bytes = 0u64;

    if let Ok(m_dir) = model_dir(app) {
        if let Ok(entries) = std::fs::read_dir(&m_dir) {
            for entry in entries.flatten() {
                if let Ok(meta) = entry.metadata() {
                    total_bytes += meta.len();
                }
            }
        }
    }

    if let Ok(r_dir) = runtime_dir(app) {
        let _ = count_dir_bytes(&r_dir, &mut total_bytes);
    }

    let size_mb = total_bytes / (1024 * 1024);

    ParakeetStatus {
        installed,
        is_downloading: IS_DOWNLOADING.load(Ordering::Relaxed),
        size_mb,
        runtime_installed: runtime_ok,
        model_installed: model_ok,
        model_dir: model_dir(app).ok().map(|p| p.to_string_lossy().to_string()),
    }
}

fn count_dir_bytes(dir: &Path, acc: &mut u64) -> std::io::Result<()> {
    if dir.is_dir() {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                let _ = count_dir_bytes(&path, acc);
            } else if let Ok(meta) = entry.metadata() {
                *acc += meta.len();
            }
        }
    }
    Ok(())
}

#[derive(Clone, Serialize)]
pub struct ParakeetDownloadProgress {
    pub step: String,
    pub downloaded: u64,
    pub total: u64,
    pub percentage: f32,
}

#[derive(Clone, Serialize)]
pub struct ParakeetDownloadError {
    pub error: String,
}

/// Dispara o download e instalação em background.
pub fn spawn_download<R: Runtime>(app: AppHandle<R>) -> Result<()> {
    if IS_DOWNLOADING.swap(true, Ordering::SeqCst) {
        return Err(anyhow!("Download do modelo Parakeet-TDT já está em andamento"));
    }

    std::thread::Builder::new()
        .name("parakeet-downloader".into())
        .spawn(move || {
            let result = run_download_pipeline(&app);
            IS_DOWNLOADING.store(false, Ordering::SeqCst);

            match result {
                Ok(_) => {
                    let _ = app.emit("parakeet-download-complete", ());
                }
                Err(e) => {
                    let _ = app.emit(
                        "parakeet-download-error",
                        ParakeetDownloadError {
                            error: format!("{:#}", e),
                        },
                    );
                }
            }
        })
        .context("falha ao iniciar thread de download do Parakeet-TDT")?;

    Ok(())
}

fn run_download_pipeline<R: Runtime>(app: &AppHandle<R>) -> Result<()> {
    let r_dir = runtime_dir(app)?;
    let m_dir = model_dir(app)?;

    let (http_proxy, danger_accept_invalid_certs) =
        match app.try_state::<crate::config::SharedConfig>() {
            Some(state) => state
                .lock()
                .map(|g| (g.http_proxy.trim().to_string(), g.danger_accept_invalid_certs))
                .unwrap_or_else(|_| (String::new(), false)),
            None => (String::new(), false),
        };

    let mut builder = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(900));

    if !http_proxy.is_empty() {
        if let Ok(proxy) = reqwest::Proxy::all(&http_proxy) {
            builder = builder.proxy(proxy);
        }
    }
    if danger_accept_invalid_certs {
        builder = builder.danger_accept_invalid_certs(true);
    }

    let client = builder
        .build()
        .context("falha ao criar client HTTP para download do Parakeet")?;

    // 1. Download do runtime sherpa-onnx se necessário (~21MB)
    if !is_runtime_installed(app) {
        let _ = app.emit(
            "parakeet-download-progress",
            ParakeetDownloadProgress {
                step: "Baixando motor de execução sherpa-onnx…".into(),
                downloaded: 0,
                total: 22_000_000,
                percentage: 0.0,
            },
        );

        let archive_part = r_dir.join("sherpa_runtime.tar.bz2.part");
        let archive_file = r_dir.join("sherpa_runtime.tar.bz2");

        let mut resp = client
            .get(SHERPA_RUNTIME_URL)
            .send()
            .context("falha ao conectar ao release do sherpa-onnx")?;

        if !resp.status().is_success() {
            return Err(anyhow!(
                "Download do runtime sherpa retornou HTTP {}",
                resp.status()
            ));
        }

        let runtime_total = resp.content_length().unwrap_or(22_000_000);
        let mut file = File::create(&archive_part)?;
        let mut downloaded = 0u64;
        let mut buf = [0u8; 64 * 1024];
        let mut last_emit = Instant::now();

        loop {
            let n = resp.read(&mut buf)?;
            if n == 0 {
                break;
            }
            file.write_all(&buf[..n])?;
            downloaded += n as u64;

            if last_emit.elapsed().as_millis() >= 250 {
                let pct = (downloaded as f32 / runtime_total as f32 * 100.0).clamp(0.0, 100.0);
                let _ = app.emit(
                    "parakeet-download-progress",
                    ParakeetDownloadProgress {
                        step: "Baixando motor de execução sherpa-onnx…".into(),
                        downloaded,
                        total: runtime_total,
                        percentage: pct,
                    },
                );
                last_emit = Instant::now();
            }
        }
        file.flush()?;
        drop(file);

        if archive_file.exists() {
            let _ = std::fs::remove_file(&archive_file);
        }
        std::fs::rename(&archive_part, &archive_file)?;

        // Extrai o arquivo tar.bz2
        let _ = app.emit(
            "parakeet-download-progress",
            ParakeetDownloadProgress {
                step: "Extraindo motor sherpa-onnx…".into(),
                downloaded: runtime_total,
                total: runtime_total,
                percentage: 100.0,
            },
        );

        let mut tar_cmd = std::process::Command::new("tar");
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            tar_cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }
        tar_cmd.arg("-xf").arg(&archive_file).arg("-C").arg(&r_dir);

        let out = tar_cmd
            .output()
            .context("falha ao executar tar para extrair runtime sherpa")?;
        if !out.status.success() {
            return Err(anyhow!(
                "erro na extração do runtime: {}",
                String::from_utf8_lossy(&out.stderr)
            ));
        }

        let _ = std::fs::remove_file(&archive_file);
    }

    // 2. Download dos pesos do modelo (~670MB no total)
    let total_model_bytes: u64 = MODEL_FILES.iter().map(|f| f.approx_size_bytes).sum();
    let mut accumulated_downloaded = 0u64;

    for item in MODEL_FILES {
        let dest_path = m_dir.join(item.name);
        if dest_path.exists() {
            if let Ok(meta) = dest_path.metadata() {
                if meta.len() > 0 && (!item.name.starts_with("encoder") || meta.len() > 500_000_000) {
                    accumulated_downloaded += meta.len();
                    continue;
                }
            }
        }

        let part_path = m_dir.join(format!("{}.part", item.name));
        let primary_url = format!("{}{}", HF_BASE_URL, item.hf_path);
        let mirror_url = format!("{}{}", HF_MIRROR_BASE_URL, item.hf_path);

        let mut resp = match client.get(&primary_url).send() {
            Ok(r) if r.status().is_success() => r,
            _ => client
                .get(&mirror_url)
                .send()
                .with_context(|| format!("falha ao conectar para baixar {}", item.name))?,
        };

        if !resp.status().is_success() {
            return Err(anyhow!(
                "download de {} retornou HTTP {}",
                item.name,
                resp.status()
            ));
        }

        let _file_total = resp.content_length().unwrap_or(item.approx_size_bytes);
        let mut file = File::create(&part_path)?;
        let mut file_downloaded = 0u64;
        let mut buf = [0u8; 128 * 1024]; // 128KB chunks
        let mut last_emit = Instant::now();

        loop {
            let n = resp.read(&mut buf)?;
            if n == 0 {
                break;
            }
            file.write_all(&buf[..n])?;
            file_downloaded += n as u64;

            if last_emit.elapsed().as_millis() >= 250 {
                let current_overall = accumulated_downloaded + file_downloaded;
                let pct = (current_overall as f32 / total_model_bytes as f32 * 100.0).clamp(0.0, 100.0);
                let _ = app.emit(
                    "parakeet-download-progress",
                    ParakeetDownloadProgress {
                        step: format!("Baixando modelo Parakeet-TDT: {}…", item.name),
                        downloaded: current_overall,
                        total: total_model_bytes,
                        percentage: pct,
                    },
                );
                last_emit = Instant::now();
            }
        }
        file.flush()?;
        drop(file);

        if dest_path.exists() {
            let _ = std::fs::remove_file(&dest_path);
        }
        std::fs::rename(&part_path, &dest_path)?;
        accumulated_downloaded += file_downloaded;
    }

    Ok(())
}

/// Exclui os arquivos do modelo Parakeet-TDT para liberar espaço em disco.
pub fn delete_parakeet_model<R: Runtime>(app: &AppHandle<R>) -> Result<()> {
    if let Ok(dir) = model_dir(app) {
        if dir.exists() {
            std::fs::remove_dir_all(&dir)?;
        }
    }
    Ok(())
}

/// Estrutura de saída JSON retornada pelo sherpa-onnx-offline.
#[derive(Debug, Deserialize)]
struct SherpaJsonOutput {
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub tokens: Vec<String>,
    #[serde(default)]
    pub timestamps: Vec<f32>,
}

/// Executa a transcrição local com o NVIDIA Parakeet-TDT 0.6B v3.
pub fn transcribe_parakeet<R: Runtime>(
    app: &AppHandle<R>,
    audio_path: &Path,
) -> Result<TranscriptionFileResult> {
    if !is_runtime_installed(app) || !is_model_installed(app) {
        return Err(anyhow!(
            "O modelo NVIDIA Parakeet-TDT v3 ainda não está instalado. Acesse Configurações > Transcrição para fazer o download."
        ));
    }

    let cli_path = get_sherpa_cli_path(app)
        .context("executável sherpa-onnx-offline não encontrado")?;
    let m_dir = model_dir(app)?;

    let tokens_path = m_dir.join("tokens.txt");
    let encoder_path = m_dir.join("encoder.int8.onnx");
    let decoder_path = m_dir.join("decoder.int8.onnx");
    let joiner_path = m_dir.join("joiner.int8.onnx");

    // 1. Decodifica o áudio de entrada para PCM mono 16kHz
    let decoded = crate::audio_decode::decode_audio_file(audio_path)?;

    // 2. Grava arquivo WAV temporário de 16kHz mono 16-bit
    let rand_id = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let temp_wav = std::env::temp_dir().join(format!("parakeet_input_{}.wav", rand_id));

    let wav_spec = hound::WavSpec {
        channels: 1,
        sample_rate: 16000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };

    {
        let mut writer = hound::WavWriter::create(&temp_wav, wav_spec)
            .context("falha ao criar arquivo wav temporário para inferência")?;
        for &sample in &decoded.samples {
            let clamped = sample.clamp(-1.0, 1.0);
            let s16 = (clamped * 32767.0) as i16;
            writer.write_sample(s16)?;
        }
        writer.finalize()?;
    }

    // 3. Monta comando do sherpa-onnx-offline
    let available_threads = std::thread::available_parallelism()
        .map(|n| n.get() as i32)
        .unwrap_or(4);
    let num_threads = available_threads.clamp(2, 8);

    let mut cmd = std::process::Command::new(&cli_path);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }

    if let Some(parent) = cli_path.parent() {
        cmd.current_dir(parent);
    }

    cmd.arg(format!("--tokens={}", tokens_path.display()));
    cmd.arg(format!("--encoder={}", encoder_path.display()));
    cmd.arg(format!("--decoder={}", decoder_path.display()));
    cmd.arg(format!("--joiner={}", joiner_path.display()));
    cmd.arg("--model-type=nemo_transducer");
    cmd.arg(format!("--num-threads={}", num_threads));
    cmd.arg("--provider=cpu");
    cmd.arg(&temp_wav);

    let output_res = cmd.output();
    let _ = std::fs::remove_file(&temp_wav);

    let output = output_res.context("falha ao executar processo do Parakeet-TDT")?;

    if !output.status.success() {
        let err_str = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("Erro ao transcrever com Parakeet: {}", err_str));
    }

    let stdout_str = String::from_utf8_lossy(&output.stdout);
    let clean = stdout_str.trim();

    let json_str = if let Some(start) = clean.find('{') {
        if let Some(end) = clean.rfind('}') {
            &clean[start..=end]
        } else {
            clean
        }
    } else {
        clean
    };

    let parsed: SherpaJsonOutput = serde_json::from_str(json_str)
        .with_context(|| format!("falha ao interpretar JSON de resposta do Parakeet: {}", clean))?;

    // 4. Monta os segmentos e detecta separação de oradores via <|spkchange|> e pontuação
    let segments = build_segments_from_tokens(&parsed.tokens, &parsed.timestamps, decoded.duration_sec);

    let full_text = if !parsed.text.is_empty() {
        parsed.text
    } else {
        segments
            .iter()
            .map(|s| s.text.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    };

    Ok(TranscriptionFileResult {
        text: clean_special_tokens(&full_text),
        duration_sec: decoded.duration_sec,
        segments,
    })
}

/// Limpa tokens especiais da transcrição final.
fn clean_special_tokens(s: &str) -> String {
    let mut out = s.to_string();
    for token in &[
        "<|startoftranscript|>",
        "<|pnc|>",
        "<|nopnc|>",
        "<|notimestamp|>",
        "<|timestamp|>",
        "<|itn|>",
        "<|noitn|>",
        "<|diarize|>",
        "<|nodiarize|>",
        "<unk>",
        "<pad>",
        "<|endoftext|>",
        "<|audioseparator|>",
        "<|nospeech|>",
        "<|emo:undefined|>",
        "<|emo:neutral|>",
        "<|emo:happy|>",
        "<|emo:sad|>",
    ] {
        out = out.replace(token, "");
    }
    // Troca marcador de orador por quebra limpa
    out = out.replace("<|spkchange|>", "\n[Novo Orador]: ");
    out.trim().to_string()
}

/// Reconstrói segmentos cronológicos respeitando trocas de orador (<|spkchange|>), pausas e perguntas.
fn build_segments_from_tokens(
    tokens: &[String],
    timestamps: &[f32],
    audio_duration: f32,
) -> Vec<TranscriptSegment> {
    let mut segments = Vec::new();
    if tokens.is_empty() {
        return segments;
    }

    let mut current_text = String::new();
    let mut current_start = 0.0f32;
    let mut current_end = 0.0f32;
    let mut is_first_token = true;

    for (i, raw_tok) in tokens.iter().enumerate() {
        let tok = raw_tok.as_str();

        // Se for token de troca de orador: encerra o turno atual imediatamente
        if tok == "<|spkchange|>" {
            let trimmed = clean_special_tokens(&current_text);
            if !trimmed.is_empty() {
                segments.push(TranscriptSegment {
                    start_sec: current_start,
                    end_sec: current_end.max(current_start + 0.3),
                    text: trimmed,
                });
            }
            current_text.clear();
            is_first_token = true;
            continue;
        }

        // Ignora tokens de controle sem texto
        if tok.starts_with("<|") && tok.ends_with("|>") {
            continue;
        }
        if tok == "<unk>" || tok == "<pad>" {
            continue;
        }

        let ts = timestamps.get(i).copied().unwrap_or(current_end);

        // Se houver uma pausa sensível no áudio (> 0.65s) e já tivermos texto acumulado, encerra o turno anterior
        if !is_first_token && ts > current_end + 0.65 && !current_text.trim().is_empty() {
            let trimmed = clean_special_tokens(&current_text);
            if !trimmed.is_empty() {
                segments.push(TranscriptSegment {
                    start_sec: current_start,
                    end_sec: current_end.max(current_start + 0.3),
                    text: trimmed,
                });
            }
            current_text.clear();
            is_first_token = true;
        }

        if is_first_token {
            current_start = ts;
            is_first_token = false;
        }
        current_end = ts;

        // Limpa símbolo de espaço do SentencePiece se presente (  ou espaço normal)
        let clean_tok = tok.replace(' ', " ");
        current_text.push_str(&clean_tok);

        // Quebra imediata em perguntas ('?') pois indicam passagem de turno/pergunta no diálogo
        let is_question = tok.ends_with('?') || clean_tok.ends_with('?');
        // Quebra em frases completas (. ou !) com pelo menos 2s de fala acumulada
        let is_sentence_end = tok.ends_with('.') || tok.ends_with('!');
        let duration_so_far = current_end - current_start;

        if is_question || (is_sentence_end && duration_so_far >= 2.0) {
            let trimmed = clean_special_tokens(&current_text);
            if !trimmed.is_empty() {
                segments.push(TranscriptSegment {
                    start_sec: current_start,
                    end_sec: current_end.max(current_start + 0.3),
                    text: trimmed,
                });
            }
            current_text.clear();
            is_first_token = true;
        }
    }

    // Adiciona o restante pendente
    let trimmed = clean_special_tokens(&current_text);
    if !trimmed.is_empty() {
        segments.push(TranscriptSegment {
            start_sec: current_start,
            end_sec: current_end.max(audio_duration).max(current_start + 0.3),
            text: trimmed,
        });
    }

    segments
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clean_special_tokens() {
        let input = "<|startoftranscript|><|pnc|> Olá mundo!<|endoftext|>";
        let cleaned = clean_special_tokens(input);
        assert_eq!(cleaned, "Olá mundo!");

        let spk = "Primeira fala.<|spkchange|> Segunda fala.";
        let cleaned_spk = clean_special_tokens(spk);
        assert!(cleaned_spk.contains("[Novo Orador]:"));
    }

    #[test]
    fn test_build_segments_from_tokens_speaker_change() {
        let tokens = vec![
            "<|startoftranscript|>".to_string(),
            " Bom".to_string(),
            " dia".to_string(),
            ".".to_string(),
            "<|spkchange|>".to_string(),
            " Olá,".to_string(),
            " como".to_string(),
            " vai?".to_string(),
        ];
        let timestamps = vec![0.0, 0.2, 0.6, 0.9, 1.2, 1.5, 1.8, 2.2];
        let segments = build_segments_from_tokens(&tokens, &timestamps, 2.5);

        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].text, "Bom dia.");
        assert_eq!(segments[1].text, "Olá, como vai?");
        assert!(segments[0].end_sec <= segments[1].start_sec);
    }
}
