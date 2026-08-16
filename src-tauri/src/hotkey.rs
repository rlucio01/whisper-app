//! Registro e handler dos atalhos globais.
//!
//! Dois modos, cada um com seu próprio atalho configurável:
//!   - **Push-to-talk** (`hotkey`) — segura pra gravar, solta pra parar.
//!     Sempre ativo; usa `DEFAULT_HOTKEY` se o campo estiver vazio.
//!   - **Hands-free** (`hands_free_hotkey`) — toque pra começar, toque de
//!     novo pra parar (não precisa segurar). Opcional — vazio = desativado.
//!
//! Os dois modos compartilham um único flag (`SharedRecordingActive`): tanto
//! iniciar quanto parar, seja por qual atalho for, lê/escreve o mesmo estado.
//! Isso faz misturar os dois modos se comportar de forma previsível — por
//! exemplo, soltar o push-to-talk sempre encerra a gravação em curso, mesmo
//! que ela tenha começado via hands-free — em vez de cada modo ter seu
//! próprio estado interno que pode dessincronizar do outro.
//!
//! A string aceita o mesmo formato do accelerator do Tauri/Electron: `"F9"`,
//! `"CommandOrControl+Shift+K"`, `"Alt+Space"` etc. O parsing é feito pelo
//! próprio plugin (`Shortcut::from_str`).
//!
//! Fluxo:
//!   - `register()` roda uma vez no `setup()`, lendo o config atual.
//!   - `sync()` é chamado pelo comando `save_config` quando qualquer um dos
//!     dois atalhos muda — desregistra tudo e re-registra os dois a partir
//!     do config novo. Se algum dos dois for inválido, nada é alterado (os
//!     atalhos antigos continuam ativos e o config não é corrompido).

use std::str::FromStr;
use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Emitter, Manager, Runtime};
use tauri_plugin_global_shortcut::{
    GlobalShortcutExt, Shortcut, ShortcutEvent, ShortcutState,
};

use crate::active_app::{self, SharedActiveApp};
use crate::audio::AudioService;
use crate::config::{AppConfig, SharedConfig};
use crate::sound;
use crate::visual;

/// String usada quando o config não tem push-to-talk configurado.
pub const DEFAULT_HOTKEY: &str = "F9";

/// `true` enquanto uma gravação está em curso, iniciada por qualquer um dos
/// dois atalhos. Ver comentário do módulo para o porquê de ser compartilhado.
pub type SharedRecordingActive = Arc<Mutex<bool>>;

/// Alias para o tipo de erro genérico que o `setup()` do Tauri aceita.
type SetupResult = Result<(), Box<dyn std::error::Error>>;

/// Registra os atalhos configurados no config atual. Chamado uma vez no
/// `setup()` — depois disso, `sync()` cuida das trocas via settings.
pub fn register<R: Runtime>(app: &AppHandle<R>) -> SetupResult {
    let cfg = read_config_snapshot(app);
    sync(app, &cfg)?;
    Ok(())
}

/// Desregistra tudo e re-registra os dois atalhos (push-to-talk + hands-free,
/// esse último só se não estiver vazio) a partir de `cfg`. Valida os dois
/// ANTES de desregistrar qualquer coisa — se algum for inválido, os atalhos
/// anteriores continuam ativos e nada muda.
pub fn sync<R: Runtime>(app: &AppHandle<R>, cfg: &AppConfig) -> Result<(), String> {
    let ptt_string = if cfg.hotkey.trim().is_empty() {
        DEFAULT_HOTKEY.to_string()
    } else {
        cfg.hotkey.trim().to_string()
    };
    let ptt_shortcut = parse_hotkey(&ptt_string)?;

    let hf_string = cfg.hands_free_hotkey.trim();
    let hf_shortcut = if hf_string.is_empty() {
        None
    } else {
        if hf_string.eq_ignore_ascii_case(&ptt_string) {
            return Err(format!(
                "o atalho hands-free não pode ser igual ao push-to-talk (\"{}\")",
                ptt_string
            ));
        }
        Some(parse_hotkey(hf_string)?)
    };

    let gs = app.global_shortcut();
    gs.unregister_all()
        .map_err(|e| format!("falha ao desregistrar atalhos anteriores: {}", e))?;

    gs.on_shortcut(ptt_shortcut, handler())
        .map_err(|e| format!("falha ao registrar atalho push-to-talk: {}", e))?;

    if let Some(shortcut) = hf_shortcut {
        gs.on_shortcut(shortcut, hands_free_handler())
            .map_err(|e| format!("falha ao registrar atalho hands-free: {}", e))?;
    }

    Ok(())
}

/// Pausa temporariamente todos os atalhos globais. Usado pela UI de settings
/// enquanto está capturando uma nova combinação — sem isso, se o usuário
/// apertar um atalho já ativo durante a captura, o app reage a ele.
pub fn pause<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    app.global_shortcut()
        .unregister_all()
        .map_err(|e| format!("falha ao pausar atalhos: {}", e))
}

/// Restaura os atalhos lendo do config atual (os valores que estavam ativos
/// antes da pausa). Chamado pela UI ao cancelar/finalizar a captura.
pub fn resume<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    let cfg = read_config_snapshot(app);
    sync(app, &cfg)
}

/// Faz parse de uma string de accelerator. Aceita `"F9"`, `"Ctrl+Shift+K"`, etc.
/// Retorna erro amigável (`String`) para propagar até a UI.
pub fn parse_hotkey(s: &str) -> Result<Shortcut, String> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return Err("combinação de tecla vazia".to_string());
    }
    Shortcut::from_str(trimmed)
        .map_err(|e| format!("combinação inválida \"{}\": {}", trimmed, e))
}

/// Snapshot do config atual, ou default se o state não estiver disponível
/// (nunca deveria acontecer em produção, mas evita panic).
fn read_config_snapshot<R: Runtime>(app: &AppHandle<R>) -> AppConfig {
    app.try_state::<SharedConfig>()
        .and_then(|s| s.lock().ok().map(|g| g.clone()))
        .unwrap_or_default()
}

/// Handler do push-to-talk: segura grava, solta para.
fn handler<R: Runtime>(
) -> impl Fn(&AppHandle<R>, &Shortcut, ShortcutEvent) + Send + Sync + 'static {
    |app, _sc, event| match event.state() {
        ShortcutState::Pressed => begin_recording(app),
        ShortcutState::Released => end_recording(app),
    }
}

/// Handler do hands-free: alterna começar/parar a cada toque. Só reage ao
/// `Pressed` — o `Released` do toque é ignorado (não é modo "segurar").
fn hands_free_handler<R: Runtime>(
) -> impl Fn(&AppHandle<R>, &Shortcut, ShortcutEvent) + Send + Sync + 'static {
    |app, _sc, event| {
        if event.state() != ShortcutState::Pressed {
            return;
        }
        let Some(state) = app.try_state::<SharedRecordingActive>() else {
            return;
        };
        let is_active = state.lock().map(|g| *g).unwrap_or(false);
        if is_active {
            end_recording(app);
        } else {
            begin_recording(app);
        }
    }
}

/// Início de uma gravação — comum aos dois modos. Detecta o app em foco,
/// marca o flag compartilhado, notifica a UI e dispara a captura de áudio.
fn begin_recording<R: Runtime>(app: &AppHandle<R>) {
    if let Some(state) = app.try_state::<SharedRecordingActive>() {
        if let Ok(mut active) = state.lock() {
            *active = true;
        }
    }

    // Captura o app em foco ANTES de qualquer coisa. Se o usuário trocar de
    // janela depois, ainda assim usamos o app que estava ativo no momento
    // em que ele decidiu ditar.
    let detected = active_app::detect();
    if let Some(state) = app.try_state::<SharedActiveApp>() {
        if let Ok(mut guard) = state.lock() {
            *guard = detected;
        }
    }

    let _ = app.emit("hotkey-pressed", ());
    visual::set(app, visual::State::Recording);
    sound::play(app, sound::Kind::Start);
    app.state::<AudioService>().start();
}

/// Fim de uma gravação — comum aos dois modos.
fn end_recording<R: Runtime>(app: &AppHandle<R>) {
    if let Some(state) = app.try_state::<SharedRecordingActive>() {
        if let Ok(mut active) = state.lock() {
            *active = false;
        }
    }

    let _ = app.emit("hotkey-released", ());
    app.state::<AudioService>().stop();
}
