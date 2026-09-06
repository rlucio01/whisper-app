//! Módulo de gerenciamento e execução da Aceleração Local por GPU (NVIDIA CUDA).
//!
//! Permite que computadores com placa de vídeo NVIDIA executem a transcrição
//! localmente na GPU através do runtime dedicado `whisper.cpp` com cuBLAS.
//!
//! O runtime é baixado sob demanda para:
//!   `%APPDATA%\com.rlucio.whisperapp\runtimes\cuda\`

use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, Runtime};

/// Flag atômico para evitar disparos simultâneos de download do runtime.
static IS_DOWNLOADING: AtomicBool = AtomicBool::new(false);

/// URL oficial da release do whisper.cpp compilada com CUDA 12.4 e cuBLAS.
const CUDA_RUNTIME_URL: &str =
    "https://github.com/ggml-org/whisper.cpp/releases/download/b4938/whisper-cublas-12.4.0-bin-x64.zip";

/// Retorna o diretório onde o runtime CUDA fica instalado.
pub fn runtime_dir<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf> {
    let app_dir = app
        .path()
        .app_data_dir()
        .context("falha ao obter app data dir")?;
    let path = app_dir.join("runtimes").join("cuda");
    std::fs::create_dir_all(&path)
        .with_context(|| format!("falha ao criar pasta de runtime em {}", path.display()))?;
    Ok(path)
}

/// Localiza o executável `whisper-cli.exe` dentro da pasta do runtime.
pub fn get_whisper_cli_path<R: Runtime>(app: &AppHandle<R>) -> Option<PathBuf> {
    let Ok(dir) = runtime_dir(app) else {
        return None;
    };

    // O zip pode extrair diretamente ou dentro de uma subpasta "Release"
    let direct = dir.join("whisper-cli.exe");
    if direct.exists() {
        return Some(direct);
    }

    let sub_release = dir.join("Release").join("whisper-cli.exe");
    if sub_release.exists() {
        return Some(sub_release);
    }

    None
}

/// Verifica se o runtime da GPU está instalado e pronto para execução.
pub fn is_cuda_runtime_installed<R: Runtime>(app: &AppHandle<R>) -> bool {
    let Some(cli) = get_whisper_cli_path(app) else {
        return false;
    };
    let dir = cli.parent().unwrap_or(&cli);
    // Verifica também se o ggml-cuda.dll está junto do executável
    dir.join("ggml-cuda.dll").exists()
}

/// Informações de status do runtime de GPU para a interface.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuRuntimeStatus {
    pub is_nvidia_detected: bool,
    pub installed: bool,
    pub is_downloading: bool,
    pub size_mb: u64,
    pub cli_path: Option<String>,
}

/// Retorna o status atual do runtime para a interface.
pub fn get_status<R: Runtime>(app: &AppHandle<R>) -> GpuRuntimeStatus {
    let hw = crate::hardware::detect_hardware();
    let is_nvidia = hw
        .gpus
        .iter()
        .any(|g| g.vendor.to_lowercase().contains("nvidia"));

    let installed = is_cuda_runtime_installed(app);
    let cli = get_whisper_cli_path(app);

    let mut size_mb = 0;
    if let Ok(dir) = runtime_dir(app) {
        if let Ok(entries) = std::fs::read_dir(&dir) {
            let mut total_bytes = 0u64;
            for entry in entries.flatten() {
                if let Ok(meta) = entry.metadata() {
                    total_bytes += meta.len();
                }
            }
            size_mb = total_bytes / (1024 * 1024);
        }
    }

    GpuRuntimeStatus {
        is_nvidia_detected: is_nvidia,
        installed,
        is_downloading: IS_DOWNLOADING.load(Ordering::Relaxed),
        size_mb,
        cli_path: cli.map(|p| p.to_string_lossy().to_string()),
    }
}

#[derive(Clone, Serialize)]
struct DownloadProgress {
    downloaded: u64,
    total: u64,
}

#[derive(Clone, Serialize)]
struct DownloadError {
    error: String,
}

/// Dispara o download e instalação do pacote CUDA em background.
pub fn spawn_download<R: Runtime>(app: AppHandle<R>) -> Result<()> {
    if IS_DOWNLOADING.swap(true, Ordering::SeqCst) {
        return Err(anyhow!("Download do módulo GPU já está em andamento"));
    }

    std::thread::Builder::new()
        .name("cuda-runtime-downloader".into())
        .spawn(move || {
            let result = run_download_pipeline(&app);
            IS_DOWNLOADING.store(false, Ordering::SeqCst);

            match result {
                Ok(_) => {
                    let _ = app.emit("gpu-runtime-download-complete", ());
                }
                Err(e) => {
                    let _ = app.emit(
                        "gpu-runtime-download-error",
                        DownloadError {
                            error: format!("{:#}", e),
                        },
                    );
                }
            }
        })
        .context("falha ao iniciar thread de download do runtime GPU")?;

    Ok(())
}

fn run_download_pipeline<R: Runtime>(app: &AppHandle<R>) -> Result<()> {
    let dest_dir = runtime_dir(app)?;
    let app_dir = app
        .path()
        .app_data_dir()
        .context("falha ao obter app data dir")?;
    let runtimes_parent = app_dir.join("runtimes");
    let zip_part = runtimes_parent.join("cuda_download.zip.part");
    let zip_file = runtimes_parent.join("cuda_download.zip");

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(600))
        .build()
        .context("falha ao criar client HTTP para download do runtime")?;

    let mut response = client
        .get(CUDA_RUNTIME_URL)
        .send()
        .context("falha ao conectar ao repositório do whisper.cpp")?;

    if !response.status().is_success() {
        return Err(anyhow!(
            "Download do runtime retornou HTTP {}",
            response.status()
        ));
    }

    let total = response.content_length().unwrap_or(671_000_000);
    let mut file = File::create(&zip_part)
        .with_context(|| format!("falha ao criar {}", zip_part.display()))?;

    let mut downloaded = 0u64;
    let mut buffer = [0u8; 64 * 1024]; // 64KB chunks
    let mut last_emit = Instant::now();

    loop {
        let bytes_read = response
            .read(&mut buffer)
            .context("erro durante leitura da stream de download")?;
        if bytes_read == 0 {
            break;
        }

        file.write_all(&buffer[..bytes_read])
            .context("falha ao gravar no disco")?;
        downloaded += bytes_read as u64;

        if last_emit.elapsed().as_millis() >= 200 {
            let _ = app.emit(
                "gpu-runtime-download-progress",
                DownloadProgress { downloaded, total },
            );
            last_emit = Instant::now();
        }
    }

    file.flush()?;
    drop(file);

    // Renomeia o .part para .zip
    if zip_file.exists() {
        let _ = std::fs::remove_file(&zip_file);
    }
    std::fs::rename(&zip_part, &zip_file)
        .with_context(|| format!("falha ao renomear {} para zip", zip_part.display()))?;

    // Extrai o zip usando o tar.exe nativo do Windows
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        let mut tar_cmd = std::process::Command::new("tar.exe");
        tar_cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        tar_cmd
            .arg("-xf")
            .arg(&zip_file)
            .arg("-C")
            .arg(&dest_dir);

        let output = tar_cmd
            .output()
            .context("falha ao executar tar.exe para extrair runtime GPU")?;

        if !output.status.success() {
            let err_str = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("falha ao extrair arquivo zip do runtime: {}", err_str));
        }
    }

    // Se o zip extraiu numa pasta Release/, movemos os arquivos para a raiz de dest_dir
    let sub_release = dest_dir.join("Release");
    if sub_release.exists() && sub_release.is_dir() {
        if let Ok(entries) = std::fs::read_dir(&sub_release) {
            for entry in entries.flatten() {
                let target = dest_dir.join(entry.file_name());
                let _ = std::fs::rename(entry.path(), target);
            }
        }
        let _ = std::fs::remove_dir_all(&sub_release);
    }

    // Remove o arquivo zip para liberar os 670MB temporários
    let _ = std::fs::remove_file(&zip_file);

    if !is_cuda_runtime_installed(app) {
        return Err(anyhow!(
            "Extração concluída mas arquivos essenciais do CUDA não foram localizados"
        ));
    }

    Ok(())
}

/// Remove o runtime do disco para liberar espaço.
pub fn delete_gpu_runtime<R: Runtime>(app: &AppHandle<R>) -> Result<()> {
    let dir = runtime_dir(app)?;
    if dir.exists() {
        std::fs::remove_dir_all(&dir)
            .with_context(|| format!("falha ao apagar pasta {}", dir.display()))?;
    }
    Ok(())
}

/// Executa a transcrição diretamente no engine CUDA (`whisper-cli.exe`).
pub fn transcribe_via_gpu_engine<R: Runtime>(
    app: &AppHandle<R>,
    model_path: &Path,
    wav_path: &Path,
    language: &str,
    prompt: Option<&str>,
) -> Result<String> {
    let cli_path = get_whisper_cli_path(app)
        .ok_or_else(|| anyhow!("Runtime CUDA não encontrado no disco"))?;

    let cli_dir = cli_path
        .parent()
        .ok_or_else(|| anyhow!("caminho do CLI inválido"))?;

    #[cfg(target_os = "windows")]
    use std::os::windows::process::CommandExt;

    let mut cmd = std::process::Command::new(&cli_path);
    cmd.current_dir(cli_dir); // Importante para que carregue as DLLs locais (ggml-cuda.dll, cublas)

    #[cfg(target_os = "windows")]
    cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW

    cmd.arg("-m")
        .arg(model_path)
        .arg("-f")
        .arg(wav_path)
        .arg("-nt")
        .arg("--no-prints")
        .arg("-t")
        .arg("4");

    let lang = language.trim();
    if !lang.is_empty() {
        cmd.arg("-l").arg(lang);
    } else {
        cmd.arg("-l").arg("auto");
    }

    if let Some(p) = prompt {
        let p_trimmed = p.trim();
        if !p_trimmed.is_empty() {
            cmd.arg("--prompt").arg(p_trimmed);
        }
    }

    let output = cmd
        .output()
        .with_context(|| format!("falha ao executar {}", cli_path.display()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("whisper-cli retornou erro: {}", stderr.trim()));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let trimmed = stdout.trim().to_string();

    if trimmed.is_empty() {
        return Err(anyhow!("whisper-cli retornou texto vazio"));
    }

    Ok(trimmed)
}

/// Executa um teste rápido de inferência no engine GPU gerando métricas reais de aceleração.
pub fn run_gpu_benchmark<R: Runtime>(
    app: &AppHandle<R>,
    model_path: &Path,
) -> Result<(u64, f32)> {
    // Sintetiza 1.0s de áudio 16kHz mono num WAV temporário
    let sample_rate = 16_000u32;
    let n_samples = sample_rate as usize;
    let mut samples = Vec::with_capacity(n_samples);
    for i in 0..n_samples {
        let t = i as f32 / sample_rate as f32;
        let s = (2.0 * std::f32::consts::PI * 440.0 * t).sin() * 0.05;
        samples.push(s);
    }

    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let temp_wav = std::env::temp_dir().join(format!("whisper_bench_gpu_{}.wav", ts));

    let spec = hound::WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };

    let mut writer = hound::WavWriter::create(&temp_wav, spec)
        .with_context(|| format!("falha ao criar {}", temp_wav.display()))?;
    for s in samples {
        let clamped = s.clamp(-1.0, 1.0);
        let val = (clamped * i16::MAX as f32) as i16;
        writer.write_sample(val)?;
    }
    writer.finalize()?;

    let start = Instant::now();
    let result = transcribe_via_gpu_engine(app, model_path, &temp_wav, "pt", None);
    let _ = std::fs::remove_file(&temp_wav);

    match result {
        Ok(_) => {
            let duration = start.elapsed();
            let duration_ms = duration.as_millis() as u64;
            let duration_secs = duration.as_secs_f32().max(0.001);
            let speedup = 1.0f32 / duration_secs;
            Ok((duration_ms, speedup))
        }
        Err(e) => Err(e),
    }
}
