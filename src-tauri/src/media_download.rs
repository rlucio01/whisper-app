//! Download e extração de áudio de links da web (YouTube, TikTok, Instagram, etc.).
//!
//! Utiliza `yt-dlp` para baixar diretamente o fluxo de áudio mais leve e compatível
//! sem exigir o download do vídeo completo em alta resolução.

use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context, Result};
use tauri::{AppHandle, Manager, Runtime};

#[derive(Debug, Clone, serde::Serialize)]
pub struct DownloaderStatus {
    pub available: bool,
    pub path: Option<String>,
    pub version: Option<String>,
}

/// Retorna o nome padrão do executável do yt-dlp conforme a plataforma.
fn yt_dlp_executable_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "yt-dlp.exe"
    } else {
        "yt-dlp"
    }
}

/// URL oficial da release mais recente do canal nightly do yt-dlp no GitHub
/// (contém correções diárias imediatas para novos desafios de JS do TikTok e Instagram).
fn yt_dlp_download_url() -> &'static str {
    if cfg!(target_os = "windows") {
        "https://github.com/yt-dlp/yt-dlp-nightly-builds/releases/latest/download/yt-dlp.exe"
    } else if cfg!(target_os = "macos") {
        "https://github.com/yt-dlp/yt-dlp-nightly-builds/releases/latest/download/yt-dlp_macos"
    } else {
        "https://github.com/yt-dlp/yt-dlp-nightly-builds/releases/latest/download/yt-dlp"
    }
}

/// Executa um comando silenciosamente sem abrir janela no Windows.
fn create_silent_command(path: &Path) -> Command {
    let mut cmd = Command::new(path);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    cmd
}

/// Tenta obter a versão do yt-dlp apontado por `path`.
fn test_yt_dlp(path: &Path) -> Option<String> {
    let output = create_silent_command(path).arg("--version").output().ok()?;
    if output.status.success() {
        let v = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !v.is_empty() {
            return Some(v);
        }
    }
    None
}

/// Atualiza o executável yt-dlp para a versão nightly mais recente.
pub fn update_yt_dlp(path: &Path) -> bool {
    let output = create_silent_command(path)
        .args(["--update-to", "nightly"])
        .output();
    output.map(|o| o.status.success()).unwrap_or(false)
}

/// Procura o executável do yt-dlp em múltiplos locais de forma transparente:
/// 1. Diretório de binários da aplicação (`<app_data_dir>/bin/yt-dlp.exe`)
/// 2. PATH do sistema operacional
/// 3. Diretório de Downloads do usuário (caso já tenha baixado)
pub fn find_yt_dlp<R: Runtime>(app: &AppHandle<R>) -> Option<PathBuf> {
    let exe_name = yt_dlp_executable_name();

    // 1. Diretório de binários da aplicação
    if let Ok(app_data) = app.path().app_data_dir() {
        let app_bin = app_data.join("bin").join(exe_name);
        if app_bin.exists() && test_yt_dlp(&app_bin).is_some() {
            return Some(app_bin);
        }
    }

    // 2. PATH do sistema
    let direct_cmd = PathBuf::from(exe_name);
    if test_yt_dlp(&direct_cmd).is_some() {
        return Some(direct_cmd);
    }

    // 3. Pastas comuns do usuário (Windows: Downloads, LocalAppData/yt-dlp)
    #[cfg(target_os = "windows")]
    {
        if let Ok(userprofile) = std::env::var("USERPROFILE") {
            let user_dl = PathBuf::from(&userprofile).join("Downloads").join(exe_name);
            if user_dl.exists() && test_yt_dlp(&user_dl).is_some() {
                // Copia para app_data para manter gerência interna
                if let Ok(app_data) = app.path().app_data_dir() {
                    let bin_dir = app_data.join("bin");
                    let _ = std::fs::create_dir_all(&bin_dir);
                    let target = bin_dir.join(exe_name);
                    if !target.exists() {
                        let _ = std::fs::copy(&user_dl, &target);
                        if target.exists() && test_yt_dlp(&target).is_some() {
                            return Some(target);
                        }
                    }
                }
                return Some(user_dl);
            }
        }
        if let Ok(localappdata) = std::env::var("LOCALAPPDATA") {
            let appdata_bin = PathBuf::from(&localappdata).join("yt-dlp").join(exe_name);
            if appdata_bin.exists() && test_yt_dlp(&appdata_bin).is_some() {
                return Some(appdata_bin);
            }
        }
    }

    None
}

/// Retorna o status de disponibilidade do yt-dlp.
pub fn get_downloader_status<R: Runtime>(app: &AppHandle<R>) -> DownloaderStatus {
    if let Some(path) = find_yt_dlp(app) {
        let version = test_yt_dlp(&path);
        DownloaderStatus {
            available: true,
            path: Some(path.to_string_lossy().to_string()),
            version,
        }
    } else {
        DownloaderStatus {
            available: false,
            path: None,
            version: None,
        }
    }
}

/// Baixa o executável autônomo do yt-dlp e o armazena em `<app_data_dir>/bin/`.
pub fn install_yt_dlp<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf> {
    let app_data = app
        .path()
        .app_data_dir()
        .context("falha ao resolver app_data_dir")?;
    let bin_dir = app_data.join("bin");
    std::fs::create_dir_all(&bin_dir)
        .with_context(|| format!("falha ao criar pasta de binários em {:?}", bin_dir))?;

    let target_path = bin_dir.join(yt_dlp_executable_name());
    let url = yt_dlp_download_url();

    // Cria um client com suporte a redirecionamentos e TLS corporativo nativo
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .context("falha ao criar cliente HTTP")?;

    let mut response = client
        .get(url)
        .header("User-Agent", "whisper_app/1.1")
        .send()
        .with_context(|| format!("falha ao conectar para download de {}", url))?;

    if !response.status().is_success() {
        return Err(anyhow!(
            "Download do yt-dlp falhou com status HTTP {}",
            response.status()
        ));
    }

    let temp_download_path = bin_dir.join(format!(
        "{}.download",
        yt_dlp_executable_name()
    ));

    let mut out_file = File::create(&temp_download_path)
        .with_context(|| format!("falha ao criar arquivo temporário {:?}", temp_download_path))?;
    std::io::copy(&mut response, &mut out_file)
        .context("falha ao gravar bytes do executável")?;
    out_file.flush()?;
    drop(out_file);

    // No Linux/macOS, torna o arquivo executável
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&temp_download_path)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&temp_download_path, perms)?;
    }

    if target_path.exists() {
        let _ = std::fs::remove_file(&target_path);
    }
    std::fs::rename(&temp_download_path, &target_path)
        .with_context(|| format!("falha ao mover executável para {:?}", target_path))?;

    if test_yt_dlp(&target_path).is_none() {
        return Err(anyhow!(
            "O executável baixado do yt-dlp em {:?} não respondeu ao comando de teste.",
            target_path
        ));
    }

    Ok(target_path)
}

/// Garante que o yt-dlp esteja presente, baixando automaticamente se necessário.
pub fn ensure_yt_dlp<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf> {
    if let Some(path) = find_yt_dlp(app) {
        return Ok(path);
    }
    install_yt_dlp(app)
}

/// Baixa apenas o áudio de uma URL suportada (YouTube, TikTok, Instagram, etc.)
/// e retorna o caminho do arquivo temporário gerado.
pub fn download_audio_from_url<R: Runtime>(app: &AppHandle<R>, url: &str) -> Result<PathBuf> {
    let clean_url = url.trim();
    if !clean_url.starts_with("http://") && !clean_url.starts_with("https://") {
        return Err(anyhow!(
            "URL inválida. O link deve começar com http:// ou https://"
        ));
    }

    let yt_dlp_bin = ensure_yt_dlp(app).with_context(|| {
        "Não foi possível localizar ou baixar a ferramenta de extração de mídia (yt-dlp)."
    })?;

    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let temp_dir = std::env::temp_dir().join(format!("whisper_url_{}_{}", std::process::id(), ts));
    std::fs::create_dir_all(&temp_dir)
        .with_context(|| format!("falha ao criar pasta temporária {:?}", temp_dir))?;

    let run_download = |bin: &Path| -> std::io::Result<std::process::Output> {
        let mut cmd = create_silent_command(bin);
        cmd.args([
            "--no-playlist",
            "--no-warnings",
            "--max-filesize",
            "500M",
            // Prioriza fluxos apenas de áudio (m4a/aac, webm/opus) para download instantâneo
            "-f",
            "ba/b",
            "-P",
            &temp_dir.to_string_lossy(),
            "-o",
            "audio.%(ext)s",
            clean_url,
        ]);
        cmd.output()
    };

    let mut output = run_download(&yt_dlp_bin)
        .with_context(|| format!("falha ao executar {:?}", yt_dlp_bin))?;

    // Se falhar e a saída sugerir atualização (-U ou Unexpected response),
    // atualiza automaticamente para a versão nightly e tenta novamente
    if !output.status.success() {
        let err_text = String::from_utf8_lossy(&output.stderr);
        let out_text = String::from_utf8_lossy(&output.stdout);
        let combined = format!("{} {}", err_text, out_text);
        if combined.contains("-U") || combined.contains("Unexpected response") || combined.contains("update") {
            let _ = update_yt_dlp(&yt_dlp_bin);
            if let Ok(retry_output) = run_download(&yt_dlp_bin) {
                output = retry_output;
            }
        }
    }

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let msg = if !stderr.is_empty() {
            stderr
        } else if !stdout.is_empty() {
            stdout
        } else {
            format!("Código de saída: {:?}", output.status.code())
        };

        let _ = std::fs::remove_dir_all(&temp_dir);
        return Err(anyhow!("Erro ao extrair áudio da URL: {}", msg));
    }

    // Localiza o arquivo baixado no diretório temporário
    let mut downloaded_file: Option<PathBuf> = None;
    if let Ok(entries) = std::fs::read_dir(&temp_dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_file() {
                if let Some(file_name) = p.file_name().and_then(|n| n.to_str()) {
                    if file_name.starts_with("audio.") {
                        downloaded_file = Some(p);
                        break;
                    }
                }
            }
        }
    }

    match downloaded_file {
        Some(file_path) => Ok(file_path),
        None => {
            let _ = std::fs::remove_dir_all(&temp_dir);
            Err(anyhow!(
                "Nenhum arquivo de áudio foi encontrado após a extração da URL."
            ))
        }
    }
}
