//! Histórico de ditados — persiste cada transcrição finalizada num arquivo
//! JSON Lines (`history.jsonl`, uma entrada por linha) no `app_data_dir`.
//!
//! Escopo deliberadamente simples (app pessoal, ver CLAUDE.md): sem SQLite,
//! sem paginação, sem limite de retenção. `list()` carrega tudo em memória —
//! milhares de entradas de texto cabem tranquilamente em RAM. Append é
//! O(1) (abre em modo append); `delete`/`clear` reescrevem o arquivo inteiro,
//! o que é aceitável porque só acontecem por ação manual do usuário.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, Runtime};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    /// Timestamp em milissegundos desde a época Unix. Também serve de ID
    /// único — dois ditados não terminam no mesmo milissegundo na prática
    /// (o pipeline de transcrição+LLM leva pelo menos algumas centenas de ms).
    pub id: i64,
    pub raw_text: String,
    pub formatted_text: String,
}

fn history_file_path<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf> {
    let dir = app
        .path()
        .app_data_dir()
        .context("falha ao obter app data dir")?;
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("falha ao criar app data dir em {}", dir.display()))?;
    Ok(dir.join("history.jsonl"))
}

/// Adiciona uma entrada ao final do arquivo. Falha aqui é só logada — não
/// deve interromper o pipeline de ditado (o texto já foi mostrado/colado
/// independentemente de conseguirmos persistir o histórico).
pub fn append<R: Runtime>(app: &AppHandle<R>, raw_text: &str, formatted_text: &str) {
    if let Err(e) = append_inner(app, raw_text, formatted_text) {
        eprintln!("[history] falha ao salvar entrada: {:#}", e);
    }
}

fn append_inner<R: Runtime>(app: &AppHandle<R>, raw_text: &str, formatted_text: &str) -> Result<()> {
    let path = history_file_path(app)?;
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let entry = HistoryEntry {
        id,
        raw_text: raw_text.to_string(),
        formatted_text: formatted_text.to_string(),
    };
    let line = serde_json::to_string(&entry).context("falha ao serializar entrada de histórico")?;

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("falha ao abrir {}", path.display()))?;
    writeln!(file, "{}", line).context("falha ao escrever no histórico")?;
    Ok(())
}

/// Lê todas as entradas na ordem em que foram gravadas (mais antiga primeiro).
/// Linhas corrompidas são ignoradas silenciosamente — não derrubam o
/// histórico inteiro por causa de uma linha truncada (ex: crash no meio de
/// um `write`).
fn read_all<R: Runtime>(app: &AppHandle<R>) -> Result<Vec<HistoryEntry>> {
    let path = history_file_path(app)?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("falha ao ler {}", path.display()))?;
    Ok(content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect())
}

/// Lista todas as entradas, mais recente primeiro (ordem de exibição na UI).
pub fn list<R: Runtime>(app: &AppHandle<R>) -> Result<Vec<HistoryEntry>> {
    let mut entries = read_all(app)?;
    entries.reverse();
    Ok(entries)
}

/// Remove uma entrada pelo ID, reescrevendo o arquivo sem ela.
pub fn delete<R: Runtime>(app: &AppHandle<R>, id: i64) -> Result<()> {
    let path = history_file_path(app)?;
    let remaining: Vec<HistoryEntry> = read_all(app)?
        .into_iter()
        .filter(|e| e.id != id)
        .collect();
    write_all(&path, &remaining)
}

/// Apaga todo o histórico.
pub fn clear<R: Runtime>(app: &AppHandle<R>) -> Result<()> {
    let path = history_file_path(app)?;
    write_all(&path, &[])
}

fn write_all(path: &PathBuf, entries: &[HistoryEntry]) -> Result<()> {
    let mut out = String::new();
    for entry in entries {
        let line = serde_json::to_string(entry).context("falha ao serializar entrada de histórico")?;
        out.push_str(&line);
        out.push('\n');
    }
    std::fs::write(path, out).with_context(|| format!("falha ao escrever {}", path.display()))
}
