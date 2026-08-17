//! Atalhos globais compostos só por teclas modificadoras (ex: `Ctrl+Super`
//! para "Ctrl+Windows").
//!
//! A API que o `tauri-plugin-global-shortcut` usa por baixo (`RegisterHotKey`
//! no Windows) exige que toda combinação tenha pelo menos uma tecla
//! não-modificadora — "Ctrl+Windows" sozinho não é uma combinação válida
//! pra ela, não importa como a string é formatada. Apps concorrentes que
//! conseguem esse tipo de atalho usam outra abordagem: um low-level keyboard
//! hook, que recebe todo evento de tecla do sistema e decide na mão se a
//! combinação bateu. É isso que este módulo implementa, como complemento ao
//! `hotkey.rs` (que continua cuidando de combinações com tecla final via
//! `RegisterHotKey`, mais eficiente e já testado).
//!
//! ## Portabilidade
//!
//! - Windows: `SetWindowsHookExW(WH_KEYBOARD_LL, ...)` numa thread dedicada
//!   com seu próprio loop de mensagens (exigido pela API — sem isso o hook
//!   instalado nunca recebe eventos).
//! - macOS/Linux: stub. `hotkey::classify_hotkey` só tenta este parser no
//!   Windows; nos outros SOs uma combinação só de modificador cai direto no
//!   parser padrão, que rejeita com erro amigável.

use std::sync::Arc;

/// Bitmask das quatro teclas modificadoras que o parser de atalho aceita.
pub type ModSet = u8;
pub const CTRL: ModSet = 1;
pub const SHIFT: ModSet = 2;
pub const ALT: ModSet = 4;
pub const SUPER: ModSet = 8;

/// Tenta interpretar `s` como uma combinação só de modificadores (2 ou mais
/// distintos, ex: `"Ctrl+Super"`). Retorna `None` se houver qualquer tecla
/// não-modificadora ou só um modificador sozinho (fácil demais de disparar
/// sem querer) — nesses casos quem chamou deve cair no parser padrão.
pub fn parse_modifiers_only(s: &str) -> Option<ModSet> {
    let mut set: ModSet = 0;
    for raw in s.split('+') {
        let token = raw.trim();
        if token.is_empty() {
            return None;
        }
        let bit = match token.to_uppercase().as_str() {
            "CTRL" | "CONTROL" => CTRL,
            "SHIFT" => SHIFT,
            "ALT" | "OPTION" => ALT,
            "SUPER" | "CMD" | "COMMAND" | "WIN" | "WINDOWS" | "META" => SUPER,
            _ => return None,
        };
        set |= bit;
    }
    if set.count_ones() >= 2 {
        Some(set)
    } else {
        None
    }
}

/// Uma combinação que o hook deve reconhecer, com as ações a disparar.
pub struct ModTarget {
    pub mods: ModSet,
    /// Disparado na transição para "combinação totalmente pressionada".
    pub on_press: Arc<dyn Fn() + Send + Sync>,
    /// Disparado quando, estando pressionada, uma das teclas é solta. Só
    /// usado pelo push-to-talk (segura grava, solta para); hands-free e
    /// recolar reagem só ao toque (`on_press`), como já fazem no `hotkey.rs`.
    pub on_release: Option<Arc<dyn Fn() + Send + Sync>>,
}

// ---------- Windows ----------

#[cfg(target_os = "windows")]
mod imp {
    use super::{ModSet, ModTarget, ALT, CTRL, SHIFT, SUPER};
    use std::sync::{Arc, Mutex, OnceLock};
    use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_RCONTROL, VK_RMENU, VK_RSHIFT, VK_RWIN,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, GetMessageW, SetWindowsHookExW, HC_ACTION, KBDLLHOOKSTRUCT, MSG,
        WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
    };

    #[derive(Default)]
    struct Targets {
        /// Modificadores fisicamente pressionados agora, atualizado a cada
        /// evento (só a thread do hook escreve aqui).
        held: ModSet,
        push_to_talk: Option<ModTarget>,
        ptt_active: bool,
        hands_free: Option<ModTarget>,
        hf_active: bool,
        repaste: Option<ModTarget>,
        rp_active: bool,
    }

    static TARGETS: OnceLock<Mutex<Targets>> = OnceLock::new();

    fn targets() -> &'static Mutex<Targets> {
        TARGETS.get_or_init(|| Mutex::new(Targets::default()))
    }

    /// Instala o hook numa thread dedicada com seu próprio loop de mensagens.
    /// Chamado uma vez no setup do app; fica ativo até o processo terminar
    /// (o Windows desinstala o hook sozinho na saída).
    pub fn start() {
        std::thread::spawn(|| unsafe {
            let hinstance = GetModuleHandleW(std::ptr::null());
            let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), hinstance, 0);
            if hook.is_null() {
                eprintln!("modkey: falha ao instalar low-level keyboard hook");
                return;
            }
            let mut msg: MSG = std::mem::zeroed();
            // Loop de mensagens exigido pela API — sem isso o WH_KEYBOARD_LL
            // instalado nesta thread nunca recebe nenhum evento.
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {}
        });
    }

    /// Troca as combinações reconhecidas pelo hook. `None` desativa aquele
    /// slot. Reseta o estado de "pressionado" — evita falso "release" de uma
    /// combinação antiga que não existe mais.
    pub fn set_targets(
        push_to_talk: Option<ModTarget>,
        hands_free: Option<ModTarget>,
        repaste: Option<ModTarget>,
    ) {
        if let Ok(mut t) = targets().lock() {
            t.push_to_talk = push_to_talk;
            t.ptt_active = false;
            t.hands_free = hands_free;
            t.hf_active = false;
            t.repaste = repaste;
            t.rp_active = false;
            t.held = 0;
        }
    }

    pub fn clear_targets() {
        set_targets(None, None, None);
    }

    fn vk_to_bit(vk: u32) -> Option<ModSet> {
        match vk as u16 {
            VK_LCONTROL | VK_RCONTROL => Some(CTRL),
            VK_LSHIFT | VK_RSHIFT => Some(SHIFT),
            VK_LMENU | VK_RMENU => Some(ALT),
            VK_LWIN | VK_RWIN => Some(SUPER),
            _ => None,
        }
    }

    unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code == HC_ACTION as i32 {
            let info = &*(lparam as *const KBDLLHOOKSTRUCT);
            if let Some(bit) = vk_to_bit(info.vkCode) {
                let msg = wparam as u32;
                let down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
                let up = msg == WM_KEYUP || msg == WM_SYSKEYUP;
                if down || up {
                    on_modifier_event(bit, down);
                }
            }
        }
        CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam)
    }

    /// Atualiza o estado de modificadores pressionados e dispara as ações
    /// cujos alvos bateram. As closures são chamadas fora do lock (só
    /// coletadas com o lock preso) pra não segurar o mutex compartilhado
    /// durante o efeito colateral (começar gravação, colar texto, etc).
    fn on_modifier_event(bit: ModSet, down: bool) {
        let mut to_call: Vec<Arc<dyn Fn() + Send + Sync>> = Vec::new();
        {
            let Ok(mut t) = targets().lock() else {
                return;
            };
            if down {
                t.held |= bit;
            } else {
                t.held &= !bit;
            }
            let held = t.held;

            // Cada `.as_ref()...` abaixo é uma expressão isolada (termina
            // antes da linha seguinte) de propósito: `t` é um `MutexGuard`,
            // e o deref dele passa por uma chamada de método — o borrow
            // checker não consegue tratar seus campos como independentes se
            // uma referência a um campo ficar viva durante a escrita em
            // outro, como aconteceria guardando `tg: &ModTarget` num `if let`.
            if let Some(mods) = t.push_to_talk.as_ref().map(|tg| tg.mods) {
                let matches = held == mods;
                if matches && !t.ptt_active {
                    t.ptt_active = true;
                    if let Some(cb) = t.push_to_talk.as_ref().map(|tg| tg.on_press.clone()) {
                        to_call.push(cb);
                    }
                } else if !matches && t.ptt_active {
                    t.ptt_active = false;
                    if let Some(cb) = t.push_to_talk.as_ref().and_then(|tg| tg.on_release.clone()) {
                        to_call.push(cb);
                    }
                }
            }

            if let Some(mods) = t.hands_free.as_ref().map(|tg| tg.mods) {
                let matches = held == mods;
                if matches && !t.hf_active {
                    t.hf_active = true;
                    if let Some(cb) = t.hands_free.as_ref().map(|tg| tg.on_press.clone()) {
                        to_call.push(cb);
                    }
                } else if !matches {
                    t.hf_active = false;
                }
            }

            if let Some(mods) = t.repaste.as_ref().map(|tg| tg.mods) {
                let matches = held == mods;
                if matches && !t.rp_active {
                    t.rp_active = true;
                    if let Some(cb) = t.repaste.as_ref().map(|tg| tg.on_press.clone()) {
                        to_call.push(cb);
                    }
                } else if !matches {
                    t.rp_active = false;
                }
            }
        }

        for cb in to_call {
            cb();
        }
    }
}

#[cfg(target_os = "windows")]
pub use imp::{clear_targets, set_targets, start};

// ---------- macOS/Linux (stub) ----------

#[cfg(not(target_os = "windows"))]
pub fn start() {}

#[cfg(not(target_os = "windows"))]
pub fn set_targets(_push_to_talk: Option<ModTarget>, _hands_free: Option<ModTarget>, _repaste: Option<ModTarget>) {
}

#[cfg(not(target_os = "windows"))]
pub fn clear_targets() {}
