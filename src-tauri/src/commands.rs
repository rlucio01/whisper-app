//! Comandos Tauri chamáveis do frontend React via `invoke("nome", args)`.
//!
//! Esses são a "ponte" entre a UI (JS/TS) e a lógica Rust. Cada `#[tauri::command]`
//! vira uma função async no lado JS.
//!
//! Convenção: erros são retornados como `String` (não `anyhow::Error`) porque
//! só tipos serializáveis atravessam a ponte JSON pra o JS.

use tauri::{AppHandle, Runtime, State};
use tauri_plugin_autostart::ManagerExt;

use crate::audio;
use crate::config::{self, AppConfig, SharedConfig, WhisperModel};
use crate::history::{self, HistoryEntry};
use crate::hotkey;
use crate::insert;
use crate::models::{self, ModelStatus};

/// Retorna a versão do app (de `tauri.conf.json`), pra exibir na UI e
/// facilitar identificar qual build está rodando.
#[tauri::command]
pub fn get_app_version<R: Runtime>(app: AppHandle<R>) -> String {
    app.package_info().version.to_string()
}

/// Retorna o config atual (do state em memória, não relê do disco).
///
/// Usado pela tela de settings para popular os campos com os valores salvos.
#[tauri::command]
pub fn get_config(state: State<'_, SharedConfig>) -> Result<AppConfig, String> {
    let guard = state
        .lock()
        .map_err(|_| "config mutex envenenado".to_string())?;
    Ok(guard.clone())
}

/// Salva um novo config: persiste no disco E atualiza o state em memória.
/// A mudança pega imediatamente — a próxima chamada de LLM já usa o novo.
///
/// Se algum dos dois atalhos (push-to-talk ou hands-free) mudou, também
/// re-registra os atalhos globais. Se alguma combinação for inválida,
/// retornamos erro ANTES de salvar/re-registrar — os atalhos anteriores
/// continuam ativos e nada é persistido.
#[tauri::command]
pub fn save_config<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, SharedConfig>,
    new_config: AppConfig,
) -> Result<(), String> {
    // Descobre se algum atalho mudou (só re-registra se necessário — evita
    // blip nos atalhos ativos quando o usuário só mexeu em outros campos).
    let hotkeys_changed = {
        let guard = state
            .lock()
            .map_err(|_| "config mutex envenenado".to_string())?;
        guard.hotkey.trim() != new_config.hotkey.trim()
            || guard.hands_free_hotkey.trim() != new_config.hands_free_hotkey.trim()
            || guard.repaste_hotkey.trim() != new_config.repaste_hotkey.trim()
    };

    if hotkeys_changed {
        // Valida + re-registra os dois. Se falhar, retorna e nada mais é feito.
        hotkey::sync(&app, &new_config)?;
    }

    // Primeiro salva no disco. Se falhar, não atualizamos o state em memória —
    // assim os dois ficam consistentes.
    config::save(&app, &new_config).map_err(|e| format!("{:#}", e))?;

    let mut guard = state
        .lock()
        .map_err(|_| "config mutex envenenado".to_string())?;
    *guard = new_config;
    Ok(())
}

/// Pausa o atalho global. A UI de settings chama isso ao entrar em modo de
/// captura de tecla — evita que apertar o atalho atual (ex: F9) dispare a
/// gravação enquanto o usuário está configurando o novo.
#[tauri::command]
pub fn pause_hotkey<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    hotkey::pause(&app)
}

/// Restaura o atalho para o que está salvo no config (o valor pré-captura).
/// Complementa `pause_hotkey` — chamado ao sair do modo de captura.
#[tauri::command]
pub fn resume_hotkey<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    hotkey::resume(&app)
}

/// Lista todos os modelos suportados com status (baixado ou não, tamanho).
#[tauri::command]
pub fn list_whisper_models<R: Runtime>(app: AppHandle<R>) -> Result<Vec<ModelStatus>, String> {
    models::list_status(&app).map_err(|e| format!("{:#}", e))
}

/// Dispara download de um modelo em background. Retorna imediatamente;
/// o progresso vem via eventos `model-download-progress` / `-complete` / `-error`.
#[tauri::command]
pub fn download_whisper_model<R: Runtime>(app: AppHandle<R>, name: String) -> Result<(), String> {
    let m = WhisperModel::from_slug(&name)
        .ok_or_else(|| format!("modelo desconhecido: {}", name))?;
    models::spawn_download(app, m);
    Ok(())
}

/// Apaga o arquivo de um modelo baixado. Usado se o usuário quer liberar disco.
#[tauri::command]
pub fn delete_whisper_model<R: Runtime>(app: AppHandle<R>, name: String) -> Result<(), String> {
    let m = WhisperModel::from_slug(&name)
        .ok_or_else(|| format!("modelo desconhecido: {}", name))?;
    models::delete(&app, m).map_err(|e| format!("{:#}", e))
}

/// Retorna se o app está atualmente registrado para iniciar com o SO.
/// Consulta o registro do SO (não o config em memória) — assim se o usuário
/// removeu manualmente pelo Task Manager do Windows, a UI reflete a verdade.
#[tauri::command]
pub fn is_autostart_enabled<R: Runtime>(app: AppHandle<R>) -> Result<bool, String> {
    app.autolaunch()
        .is_enabled()
        .map_err(|e| format!("{:#}", e))
}

/// Ativa ou desativa o autostart. Escreve no registro do SO diretamente
/// (não passa pelo config JSON — o plugin já persiste).
#[tauri::command]
pub fn set_autostart<R: Runtime>(app: AppHandle<R>, enable: bool) -> Result<(), String> {
    let mgr = app.autolaunch();
    if enable {
        mgr.enable().map_err(|e| format!("{:#}", e))
    } else {
        mgr.disable().map_err(|e| format!("{:#}", e))
    }
}

/// Abre o file explorer na pasta que contém o `config.json`.
/// Útil para o usuário conferir onde os dados estão salvos.
#[tauri::command]
pub fn open_config_folder<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    let path = config::config_file_path(&app).map_err(|e| format!("{:#}", e))?;
    let parent = path.parent().ok_or_else(|| "path sem pai".to_string())?;

    // Usa o plugin `opener` (já registrado em `lib.rs`) para abrir no
    // gerenciador de arquivos padrão do SO.
    tauri_plugin_opener::open_path(parent, None::<&str>).map_err(|e| format!("{:#}", e))?;
    Ok(())
}

/// Lista os microfones disponíveis, pro dropdown de escolha em settings.
#[tauri::command]
pub fn list_microphones() -> Result<Vec<String>, String> {
    audio::list_devices().map_err(|e| format!("{:#}", e))
}

/// Lista o histórico de ditados, mais recente primeiro.
#[tauri::command]
pub fn get_history<R: Runtime>(app: AppHandle<R>) -> Result<Vec<HistoryEntry>, String> {
    history::list(&app).map_err(|e| format!("{:#}", e))
}

/// Apaga uma entrada específica do histórico pelo ID.
#[tauri::command]
pub fn delete_history_entry<R: Runtime>(app: AppHandle<R>, id: i64) -> Result<(), String> {
    history::delete(&app, id).map_err(|e| format!("{:#}", e))
}

/// Apaga todo o histórico de ditados.
#[tauri::command]
pub fn clear_history<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    history::clear(&app).map_err(|e| format!("{:#}", e))
}

/// Recola um texto do histórico no app ativo (mesma mecânica de colagem do
/// pipeline normal — clipboard + Ctrl/Cmd+V). Usado pelo botão "colar de
/// novo" em cada entrada do histórico.
#[tauri::command]
pub fn repaste_text(text: String) -> Result<(), String> {
    insert::paste_text(&text, None).map_err(|e| format!("{:#}", e))
}

/// Cancela a gravação em curso sem transcrever nem colar nada. Usado pelo
/// botão ✕ do painel de hover do overlay.
#[tauri::command]
pub fn cancel_recording<R: Runtime>(app: AppHandle<R>) {
    hotkey::cancel_recording(&app);
}

/// Finaliza a gravação em curso agora, como se o atalho tivesse sido
/// solto/tocado de novo — segue o pipeline normal (transcrição → LLM →
/// colar). Usado pelo botão ✓ do painel de hover do overlay.
#[tauri::command]
pub fn confirm_recording<R: Runtime>(app: AppHandle<R>) {
    hotkey::end_recording(&app);
}
