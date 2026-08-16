//! Registro e handler do atalho global (push-to-talk).
//!
//! O usuário escolhe a combinação via UI de settings. A string aceita o mesmo
//! formato do accelerator do Tauri/Electron: `"F9"`, `"CommandOrControl+Shift+K"`,
//! `"Alt+Space"` etc. O parsing é feito pelo próprio plugin (`Shortcut::from_str`).
//!
//! Fluxo:
//!   - `register()` roda uma vez no `setup()`, lendo o config atual.
//!   - `replace()` é chamado pelo comando `save_config` quando o usuário troca
//!     o atalho na UI — desregistra o anterior e registra o novo, tudo em runtime.
//!
//! Se a combinação for inválida, `replace()` erra ANTES de desregistrar o
//! anterior — o atalho antigo continua funcionando e o config não é corrompido.

use std::str::FromStr;

use tauri::{AppHandle, Emitter, Manager, Runtime};
use tauri_plugin_global_shortcut::{
    GlobalShortcutExt, Shortcut, ShortcutEvent, ShortcutState,
};

use crate::active_app::{self, SharedActiveApp};
use crate::audio::AudioService;
use crate::config::SharedConfig;
use crate::sound;
use crate::visual;

/// String usada quando o config não tem nada configurado.
pub const DEFAULT_HOTKEY: &str = "F9";

/// Alias para o tipo de erro genérico que o `setup()` do Tauri aceita.
type SetupResult = Result<(), Box<dyn std::error::Error>>;

/// Registra o atalho global lendo a combinação atual do config.
/// Chamado uma vez no `setup()` — depois disso, `replace()` cuida das trocas.
pub fn register<R: Runtime>(app: &AppHandle<R>) -> SetupResult {
    let hotkey_string = current_hotkey_string(app);
    let shortcut = parse_hotkey(&hotkey_string)?;
    app.global_shortcut().on_shortcut(shortcut, handler())?;
    Ok(())
}

/// Troca o atalho ativo por uma nova combinação em runtime.
/// Retorna erro se a string for inválida (nesse caso o atalho anterior
/// continua ativo). Idempotente se a nova combinação for igual à atual.
pub fn replace<R: Runtime>(app: &AppHandle<R>, new_hotkey: &str) -> Result<(), String> {
    // Parse ANTES de desregistrar — se falhar, não perdemos o atalho ativo.
    let new_shortcut = parse_hotkey(new_hotkey)?;

    let gs = app.global_shortcut();

    // Desregistra todos os shortcuts (só usamos um por vez, então é seguro).
    // `unregister_all` não erra se já não havia nada registrado.
    gs.unregister_all()
        .map_err(|e| format!("falha ao desregistrar atalho anterior: {}", e))?;

    gs.on_shortcut(new_shortcut, handler())
        .map_err(|e| format!("falha ao registrar novo atalho: {}", e))?;

    Ok(())
}

/// Pausa temporariamente o atalho global. Usado pela UI de settings enquanto
/// está capturando uma nova combinação — sem isso, se o usuário apertar a
/// tecla atual (ex: F9) durante a captura, o app começa a gravar.
pub fn pause<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    app.global_shortcut()
        .unregister_all()
        .map_err(|e| format!("falha ao pausar atalho: {}", e))
}

/// Restaura o atalho lendo do config atual (o valor que estava ativo antes
/// da pausa). Chamado pela UI ao cancelar/finalizar a captura.
pub fn resume<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    let hotkey_string = current_hotkey_string(app);
    let shortcut = parse_hotkey(&hotkey_string)?;
    app.global_shortcut()
        .on_shortcut(shortcut, handler())
        .map_err(|e| format!("falha ao restaurar atalho: {}", e))
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

/// Lê a string do hotkey no config atual. Se vazio ou config indisponível,
/// cai no default.
fn current_hotkey_string<R: Runtime>(app: &AppHandle<R>) -> String {
    let Some(state) = app.try_state::<SharedConfig>() else {
        return DEFAULT_HOTKEY.to_string();
    };
    let Ok(guard) = state.lock() else {
        return DEFAULT_HOTKEY.to_string();
    };
    let h = guard.hotkey.trim();
    if h.is_empty() {
        DEFAULT_HOTKEY.to_string()
    } else {
        h.to_string()
    }
}

/// Handler compartilhado entre o registro inicial e o replace.
/// Retorna um `impl Fn` novo a cada chamada — é o mesmo comportamento, mas
/// como closures não são clonáveis, produzimos um por registro.
fn handler<R: Runtime>(
) -> impl Fn(&AppHandle<R>, &Shortcut, ShortcutEvent) + Send + Sync + 'static {
    |app, _sc, event| {
        let service = app.state::<AudioService>();
        match event.state() {
            ShortcutState::Pressed => {
                // Captura o app em foco ANTES de qualquer coisa. Se o usuário
                // trocar de janela entre F9 e o release, ainda assim usamos
                // o app que estava ativo no momento em que ele decidiu ditar.
                let detected = active_app::detect();
                if let Some(state) = app.try_state::<SharedActiveApp>() {
                    if let Ok(mut guard) = state.lock() {
                        *guard = detected;
                    }
                }

                let _ = app.emit("hotkey-pressed", ());
                visual::set(app, visual::State::Recording);
                sound::play(app, sound::Kind::Start);
                service.start();
            }
            ShortcutState::Released => {
                let _ = app.emit("hotkey-released", ());
                service.stop();
            }
        }
    }
}
