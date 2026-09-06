//! Registro e handler dos atalhos globais.
//!
//! Três modos, cada um com seu próprio atalho configurável:
//!   - **Push-to-talk** (`hotkey`) — segura pra gravar, solta pra parar.
//!     Sempre ativo; usa `DEFAULT_HOTKEY` se o campo estiver vazio.
//!   - **Hands-free** (`hands_free_hotkey`) — toque pra começar, toque de
//!     novo pra parar (não precisa segurar). Opcional — vazio = desativado.
//!   - **Recolar última transcrição** (`repaste_hotkey`) — toque pra colar
//!     de novo o último texto formatado, sem regravar. Opcional.
//!
//! Os dois modos compartilham um único flag (`SharedRecordingActive`): tanto
//! iniciar quanto parar, seja por qual atalho for, lê/escreve o mesmo estado.
//! Isso faz misturar os dois modos se comportar de forma previsível — por
//! exemplo, soltar o push-to-talk sempre encerra a gravação em curso, mesmo
//! que ela tenha começado via hands-free — em vez de cada modo ter seu
//! próprio estado interno que pode dessincronizar do outro.
//!
//! A string aceita o mesmo formato do accelerator do Tauri/Electron: `"F9"`,
//! `"CommandOrControl+Shift+K"`, `"Alt+Space"` etc. — parseado pelo próprio
//! plugin (`Shortcut::from_str`) e registrado via `RegisterHotKey` (Windows).
//! Combinações só de modificador (ex: `"Ctrl+Super"`, exibida como
//! "Ctrl+Windows" na UI) não passam por `RegisterHotKey` — essa API não
//! aceita combinações sem tecla final — e vão por um caminho separado, o
//! low-level keyboard hook em `modkey.rs`. `classify_hotkey()` decide qual
//! caminho cada atalho configurado usa.
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
use crate::insert;
use crate::llm::SharedLastTranscript;
use crate::modkey::{self, ModTarget};
use crate::sound;
use crate::visual;

/// String usada quando o config não tem push-to-talk configurado.
pub const DEFAULT_HOTKEY: &str = "Ctrl+Super";

/// `true` enquanto uma gravação está em curso, iniciada por qualquer um dos
/// dois atalhos. Ver comentário do módulo para o porquê de ser compartilhado.
pub type SharedRecordingActive = Arc<Mutex<bool>>;

/// Alias para o tipo de erro genérico que o `setup()` do Tauri aceita.
type SetupResult = Result<(), Box<dyn std::error::Error>>;

/// Registra os atalhos configurados no config atual. Chamado uma vez no
/// `setup()` — depois disso, `sync()` cuida das trocas via settings.
pub fn register<R: Runtime>(app: &AppHandle<R>) -> SetupResult {
    // Instala o low-level keyboard hook (Windows) que cuida de combinações
    // só de modificador (ex: Ctrl+Windows) — RegisterHotKey não aceita esse
    // tipo de combinação, ver `modkey.rs`. Fica ativo pelo resto da vida do
    // processo; `sync()` só troca quais combinações ele reconhece.
    modkey::start();
    let cfg = read_config_snapshot(app);
    sync(app, &cfg)?;
    Ok(())
}

/// Resultado de classificar uma string de atalho: ou é uma combinação normal
/// (tem tecla final, registrável via `RegisterHotKey`/plugin), ou é só de
/// modificadores (precisa do hook em `modkey.rs`).
enum HotkeyKind {
    Standard(Shortcut),
    ModifiersOnly(modkey::ModSet),
}

/// Classifica uma string de accelerator. Aceita `"F9"`, `"Ctrl+Shift+K"` (via
/// `Shortcut::from_str`) e, só no Windows, combinações só de modificador tipo
/// `"Ctrl+Super"` (via `modkey::parse_modifiers_only`). Retorna erro amigável
/// (`String`) para propagar até a UI.
fn classify_hotkey(s: &str) -> Result<HotkeyKind, String> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return Err("combinação de tecla vazia".to_string());
    }
    // Só tenta o caminho de modificadores-só no Windows — nos outros SOs o
    // hook de `modkey.rs` é um stub, então cai no parser padrão, que rejeita
    // a combinação com uma mensagem de erro natural.
    if cfg!(target_os = "windows") {
        if let Some(mods) = modkey::parse_modifiers_only(trimmed) {
            return Ok(HotkeyKind::ModifiersOnly(mods));
        }
    }
    Shortcut::from_str(trimmed)
        .map(HotkeyKind::Standard)
        .map_err(|e| format!("combinação inválida \"{}\": {}", trimmed, e))
}

/// Desregistra tudo e re-registra os atalhos configurados (push-to-talk +
/// hands-free e recolar, esses dois últimos só se não estiverem vazios) a
/// partir de `cfg`. Valida todos ANTES de desregistrar qualquer coisa — se
/// algum for inválido ou colidir com outro, os atalhos anteriores continuam
/// ativos e nada muda.
pub fn sync<R: Runtime>(app: &AppHandle<R>, cfg: &AppConfig) -> Result<(), String> {
    let ptt_string = if cfg.hotkey.trim().is_empty() {
        DEFAULT_HOTKEY.to_string()
    } else {
        cfg.hotkey.trim().to_string()
    };
    let ptt_kind = classify_hotkey(&ptt_string)?;

    let hf_string = cfg.hands_free_hotkey.trim();
    let hf_kind = parse_optional_distinct(
        hf_string,
        "hands-free",
        &[("push-to-talk", &ptt_string)],
    )?;

    let rp_string = cfg.repaste_hotkey.trim();
    let rp_kind = parse_optional_distinct(
        rp_string,
        "de recolar última transcrição",
        &[("push-to-talk", &ptt_string), ("hands-free", hf_string)],
    )?;

    let gs = app.global_shortcut();
    gs.unregister_all()
        .map_err(|e| format!("falha ao desregistrar atalhos anteriores: {}", e))?;

    if let HotkeyKind::Standard(shortcut) = ptt_kind {
        gs.on_shortcut(shortcut, handler())
            .map_err(|e| format!("falha ao registrar atalho push-to-talk: {}", e))?;
    }

    if let Some(HotkeyKind::Standard(shortcut)) = &hf_kind {
        gs.on_shortcut(*shortcut, hands_free_handler())
            .map_err(|e| format!("falha ao registrar atalho hands-free: {}", e))?;
    }

    if let Some(HotkeyKind::Standard(shortcut)) = &rp_kind {
        gs.on_shortcut(*shortcut, repaste_handler())
            .map_err(|e| format!("falha ao registrar atalho de recolar: {}", e))?;
    }

    // Combinações só de modificador vão pro hook em vez do plugin.
    let ptt_mods = match ptt_kind {
        HotkeyKind::ModifiersOnly(mods) => {
            let app_press = app.clone();
            let app_release = app.clone();
            Some(ModTarget {
                mods,
                on_press: Arc::new(move || begin_recording(&app_press)),
                on_release: Some(Arc::new(move || end_recording(&app_release))),
            })
        }
        _ => None,
    };
    let hf_mods = match hf_kind {
        Some(HotkeyKind::ModifiersOnly(mods)) => {
            let app_press = app.clone();
            Some(ModTarget {
                mods,
                on_press: Arc::new(move || do_hands_free_toggle(&app_press)),
                on_release: None,
            })
        }
        _ => None,
    };
    let rp_mods = match rp_kind {
        Some(HotkeyKind::ModifiersOnly(mods)) => {
            let app_press = app.clone();
            Some(ModTarget {
                mods,
                on_press: Arc::new(move || do_repaste(&app_press)),
                on_release: None,
            })
        }
        _ => None,
    };
    modkey::set_targets(ptt_mods, hf_mods, rp_mods);

    Ok(())
}

/// Valida uma combinação opcional (`value` vazio = desativado, retorna
/// `None`) contra uma lista de combinações já em uso — erro amigável se
/// colidir com alguma. Usado pra `hands_free_hotkey` e `repaste_hotkey`,
/// que compartilham essa mesma regra ("não pode repetir os outros atalhos").
fn parse_optional_distinct(
    value: &str,
    label: &str,
    taken: &[(&str, &str)],
) -> Result<Option<HotkeyKind>, String> {
    if value.is_empty() {
        return Ok(None);
    }
    for (other_label, other_value) in taken {
        if value.eq_ignore_ascii_case(other_value) {
            return Err(format!(
                "o atalho {} não pode ser igual ao {} (\"{}\")",
                label, other_label, other_value
            ));
        }
    }
    Ok(Some(classify_hotkey(value)?))
}

/// Pausa temporariamente todos os atalhos globais (plugin + hook de
/// modificadores). Usado pela UI de settings enquanto está capturando uma
/// nova combinação — sem isso, se o usuário apertar um atalho já ativo
/// durante a captura, o app reage a ele.
pub fn pause<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    modkey::clear_targets();
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
        do_hands_free_toggle(app);
    }
}

/// Lógica do toque hands-free, compartilhada entre o handler do plugin
/// (combinação com tecla final) e o alvo do hook em `modkey.rs`
/// (combinação só de modificador).
fn do_hands_free_toggle<R: Runtime>(app: &AppHandle<R>) {
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

/// Handler do atalho "recolar última transcrição": pega o último texto
/// formatado com sucesso (guardado por `llm.rs`) e cola de novo no app
/// ativo, sem regravar. Só reage ao `Pressed` — é um toque, não um "segurar".
/// Se nada foi ditado ainda nessa sessão (ou desde o boot, que semeia com a
/// entrada mais recente do histórico), simplesmente não faz nada.
fn repaste_handler<R: Runtime>(
) -> impl Fn(&AppHandle<R>, &Shortcut, ShortcutEvent) + Send + Sync + 'static {
    |app, _sc, event| {
        if event.state() != ShortcutState::Pressed {
            return;
        }
        do_repaste(app);
    }
}

/// Lógica do toque "recolar", compartilhada entre o handler do plugin e o
/// alvo do hook em `modkey.rs`.
fn do_repaste<R: Runtime>(app: &AppHandle<R>) {
    let Some(state) = app.try_state::<SharedLastTranscript>() else {
        return;
    };
    let Some(text) = state.lock().ok().and_then(|g| g.clone()) else {
        return;
    };
    match insert::paste_text(&text, None) {
        Ok(()) => {
            let _ = app.emit("text-inserted", ());
            sound::play(app, sound::Kind::End);
        }
        Err(e) => {
            let _ = app.emit("insert-error", format!("{:#}", e));
        }
    }
}

/// Início de uma gravação (comum a atalho global ou acionamento local da UI).
pub fn begin_recording<R: Runtime>(app: &AppHandle<R>) {
    if let Some(state) = app.try_state::<SharedRecordingActive>() {
        if let Ok(mut active) = state.lock() {
            *active = true;
        }
    }

    // Marca no hook nativo que PTT está ativo para que soltar as teclas finalize a gravação
    modkey::mark_ptt_active(true);

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

/// Alterna início ou término da gravação.
pub fn toggle_recording<R: Runtime>(app: &AppHandle<R>) {
    do_hands_free_toggle(app);
}

/// Fim de uma gravação.
pub fn end_recording<R: Runtime>(app: &AppHandle<R>) {
    modkey::mark_ptt_active(false);

    if let Some(state) = app.try_state::<SharedRecordingActive>() {
        if let Ok(mut active) = state.lock() {
            *active = false;
        }
    }

    let _ = app.emit("hotkey-released", ());
    app.state::<AudioService>().stop();
}

/// Cancela a gravação em curso sem transcrever nem colar nada — usado pelo
/// botão ✕ do overlay. O áudio capturado até agora é descartado
/// (`AudioService::cancel`, que também esconde o overlay).
pub(crate) fn cancel_recording<R: Runtime>(app: &AppHandle<R>) {
    modkey::mark_ptt_active(false);

    if let Some(state) = app.try_state::<SharedRecordingActive>() {
        if let Ok(mut active) = state.lock() {
            *active = false;
        }
    }

    app.state::<AudioService>().cancel();
}
