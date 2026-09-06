//! Gerenciamento dos modelos do whisper.cpp: metadados, path no disco,
//! download com progresso e listagem para a UI.
//!
//! Os arquivos são baixados do repositório oficial do whisper.cpp no
//! Hugging Face e ficam em `<app_data_dir>/models/<filename>.bin`.
//!
//! ## Download
//!
//! Cada download roda em uma thread separada e emite três eventos:
//!   - `model-download-progress` → `{ name, downloaded, total }` (bytes)
//!   - `model-download-complete` → `{ name }`
//!   - `model-download-error`    → `{ name, error }`
//!
//! Se dois downloads forem disparados pro mesmo modelo em paralelo, o segundo
//! sobrescreve o primeiro no disco — sem lock elaborado por ora (o usuário
//! só clica em "Baixar" quando o botão está habilitado, então na prática
//! não acontece).

use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, Runtime};

use crate::config::WhisperModel;

/// Metadata estática de cada modelo. `filename` bate com o arquivo GGML.
pub struct ModelMeta {
    pub filename: &'static str,
    /// Lista ordenada de espelhos para download resiliente (bypassa bloqueios do HuggingFace).
    pub urls: &'static [&'static str],
    /// Tamanho aproximado em MB (para a UI mostrar antes do download).
    pub size_mb: u32,
    /// Nome amigável exibido no dropdown.
    pub display_name: &'static str,
}

impl WhisperModel {
    pub fn meta(self) -> ModelMeta {
        match self {
            WhisperModel::Tiny => ModelMeta {
                filename: "ggml-tiny-q5_1.bin",
                urls: &[
                    "https://hf-mirror.com/ggerganov/whisper.cpp/resolve/main/ggml-tiny-q5_1.bin",
                    "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny-q5_1.bin",
                ],
                size_mb: 31,
                display_name: "Tiny (~31MB): mais rápido, menos preciso",
            },
            WhisperModel::Base => ModelMeta {
                filename: "ggml-base-q5_1.bin",
                urls: &[
                    "https://hf-mirror.com/ggerganov/whisper.cpp/resolve/main/ggml-base-q5_1.bin",
                    "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base-q5_1.bin",
                ],
                size_mb: 59,
                display_name: "Base (~59MB): bom para testes",
            },
            WhisperModel::Small => ModelMeta {
                filename: "ggml-small-q5_1.bin",
                urls: &[
                    "https://hf-mirror.com/ggerganov/whisper.cpp/resolve/main/ggml-small-q5_1.bin",
                    "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small-q5_1.bin",
                ],
                size_mb: 181,
                display_name: "Small (~181MB): recomendado para uso diário",
            },
            WhisperModel::Medium => ModelMeta {
                filename: "ggml-medium-q5_0.bin",
                urls: &[
                    "https://hf-mirror.com/ggerganov/whisper.cpp/resolve/main/ggml-medium-q5_0.bin",
                    "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-medium-q5_0.bin",
                ],
                size_mb: 514,
                display_name: "Medium (~514MB): mais preciso, mais lento",
            },
            WhisperModel::LargeTurbo => ModelMeta {
                filename: "ggml-large-v3-turbo-q5_0.bin",
                urls: &[
                    "https://hf-mirror.com/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin",
                    "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin",
                ],
                size_mb: 574,
                display_name: "Large-v3 Turbo (~574MB): máxima precisão",
            },
        }
    }

    /// Slug usado para serialização em JSON (bate com `#[serde(rename_all = "snake_case")]`).
    /// Também é o que a UI envia ao chamar `download_whisper_model(name)`.
    pub fn slug(self) -> &'static str {
        match self {
            WhisperModel::Tiny => "tiny",
            WhisperModel::Base => "base",
            WhisperModel::Small => "small",
            WhisperModel::Medium => "medium",
            WhisperModel::LargeTurbo => "large_turbo",
        }
    }

    /// Lista todos os modelos suportados na ordem de tamanho.
    pub fn all() -> &'static [WhisperModel] {
        &[
            WhisperModel::Tiny,
            WhisperModel::Base,
            WhisperModel::Small,
            WhisperModel::Medium,
            WhisperModel::LargeTurbo,
        ]
    }

    /// Faz parse do slug. Usado pelos comandos Tauri que recebem `name: String`.
    pub fn from_slug(s: &str) -> Option<WhisperModel> {
        Self::all().iter().copied().find(|m| m.slug() == s)
    }
}

/// Retorna o path onde o arquivo do modelo `m` fica salvo.
/// Cria a pasta `models/` se ainda não existe.
pub fn file_path<R: Runtime>(app: &AppHandle<R>, m: WhisperModel) -> Result<PathBuf> {
    let dir = app
        .path()
        .app_data_dir()
        .context("falha ao obter app data dir")?;
    let models_dir = dir.join("models");
    fs::create_dir_all(&models_dir)
        .with_context(|| format!("falha ao criar pasta {}", models_dir.display()))?;
    Ok(models_dir.join(m.meta().filename))
}

/// Estrutura enviada para a UI listando o estado de cada modelo.
#[derive(Serialize, Deserialize)]
pub struct ModelStatus {
    pub slug: String,
    pub display_name: String,
    pub size_mb: u32,
    pub downloaded: bool,
    /// Bytes reais no disco (útil pra mostrar tamanho exato).
    pub bytes_on_disk: u64,
}

/// Lista o estado de todos os modelos. Usada pelo comando `list_whisper_models`.
pub fn list_status<R: Runtime>(app: &AppHandle<R>) -> Result<Vec<ModelStatus>> {
    let mut out = Vec::new();
    for &m in WhisperModel::all() {
        let path = file_path(app, m)?;
        let meta = m.meta();
        let (downloaded, bytes_on_disk) = match fs::metadata(&path) {
            Ok(md) => (true, md.len()),
            Err(_) => (false, 0),
        };
        out.push(ModelStatus {
            slug: m.slug().to_string(),
            display_name: meta.display_name.to_string(),
            size_mb: meta.size_mb,
            downloaded,
            bytes_on_disk,
        });
    }
    Ok(out)
}

/// Dispara o download em background. Retorna imediatamente; o progresso vem
/// via eventos Tauri.
pub fn spawn_download<R: Runtime>(app: AppHandle<R>, m: WhisperModel) {
    thread::spawn(move || {
        let slug = m.slug().to_string();
        if let Err(e) = download_blocking(&app, m) {
            let _ = app.emit(
                "model-download-error",
                DownloadError {
                    name: slug,
                    error: format!("{:#}", e),
                },
            );
        }
    });
}

/// Apaga o arquivo do modelo do disco. No-op se já não existir.
pub fn delete<R: Runtime>(app: &AppHandle<R>, m: WhisperModel) -> Result<()> {
    let path = file_path(app, m)?;
    if path.exists() {
        fs::remove_file(&path)
            .with_context(|| format!("falha ao apagar {}", path.display()))?;
    }
    Ok(())
}

// ---------- Implementação do download ----------

/// Payload dos eventos de progresso emitidos pra UI.
#[derive(Serialize, Clone)]
struct DownloadProgress {
    name: String,
    downloaded: u64,
    total: u64,
}

#[derive(Serialize, Clone)]
struct DownloadComplete {
    name: String,
}

#[derive(Serialize, Clone)]
struct DownloadError {
    name: String,
    error: String,
}

/// Baixa o arquivo, escrevendo em `<path>.part` e renomeando ao final.
/// Isso evita deixar um arquivo pela metade se o app travar no meio.
fn download_blocking<R: Runtime>(app: &AppHandle<R>, m: WhisperModel) -> Result<()> {
    let meta = m.meta();
    let final_path = file_path(app, m)?;
    let part_path = final_path.with_extension("bin.part");

    let _ = app.emit(
        "model-download-progress",
        DownloadProgress {
            name: m.slug().to_string(),
            downloaded: 0,
            total: 0,
        },
    );

    // Timeout generoso: 30min pro modelo maior (~600MB) em conexão lenta.
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(30 * 60))
        .build()
        .context("falha ao criar HTTP client")?;

    let mut response_opt = None;
    let mut last_error = String::new();

    for &url in meta.urls {
        eprintln!("[models] tentando baixar de: {}", url);
        match client.get(url).send() {
            Ok(resp) => {
                let status = resp.status();
                if status.is_success() {
                    response_opt = Some((resp, url));
                    break;
                } else {
                    last_error = format!("HTTP {} ({})", status, url);
                    eprintln!("[models] espelho retornou status {}: {}", status, url);
                }
            }
            Err(e) => {
                last_error = format!("{:#} ({})", e, url);
                eprintln!("[models] falha ao conectar no espelho {}: {:#}", url, e);
            }
        }
    }

    let (mut response, active_url) = response_opt.ok_or_else(|| {
        anyhow!(
            "falha ao baixar o modelo de todos os espelhos disponíveis (último erro: {})",
            last_error
        )
    })?;

    eprintln!("[models] baixando com sucesso a partir de: {}", active_url);

    let total = response.content_length().unwrap_or(0);
    let mut file = fs::File::create(&part_path)
        .with_context(|| format!("falha ao criar {}", part_path.display()))?;

    // Lê em chunks para poder emitir progresso periodicamente sem inundar a UI.
    // 256KB é um bom trade-off entre granularidade e overhead de emit.
    let mut buf = [0u8; 256 * 1024];
    let mut downloaded: u64 = 0;
    let mut last_emit_at: u64 = 0;

    loop {
        let n = response
            .read(&mut buf)
            .context("falha ao ler chunk do download")?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])
            .with_context(|| format!("falha ao escrever em {}", part_path.display()))?;
        downloaded += n as u64;

        // Emite progresso a cada ~1MB baixado (evita centenas de emits/s).
        if downloaded - last_emit_at >= 1024 * 1024 {
            last_emit_at = downloaded;
            let _ = app.emit(
                "model-download-progress",
                DownloadProgress {
                    name: m.slug().to_string(),
                    downloaded,
                    total,
                },
            );
        }
    }

    file.flush().context("falha ao flush do arquivo")?;
    drop(file);

    // Move `.part` → arquivo final. Se já existia (redownload), sobrescreve.
    if final_path.exists() {
        let _ = fs::remove_file(&final_path);
    }
    fs::rename(&part_path, &final_path).with_context(|| {
        format!(
            "falha ao renomear {} para {}",
            part_path.display(),
            final_path.display()
        )
    })?;

    // Emit final de progresso (100%) + complete.
    let _ = app.emit(
        "model-download-progress",
        DownloadProgress {
            name: m.slug().to_string(),
            downloaded,
            total: total.max(downloaded),
        },
    );
    let _ = app.emit(
        "model-download-complete",
        DownloadComplete {
            name: m.slug().to_string(),
        },
    );

    Ok(())
}
