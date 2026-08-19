//! Inserção do texto formatado no aplicativo ativo.
//!
//! ## Estratégia
//!
//! Clipboard + Ctrl+V (Cmd+V no macOS). Funciona em ~95% dos apps
//! cross-platform sem precisar detectar qual é o aplicativo em foco.
//!
//! Fluxo:
//!   1. Salva o conteúdo atual do clipboard (se for texto).
//!   2. Coloca o texto formatado no clipboard.
//!   3. Espera ~50ms para o clipboard propagar.
//!   4. (Windows) Restaura o foco na janela alvo, se uma foi passada.
//!   5. Simula Ctrl+V (Cmd+V no macOS).
//!   6. Espera ~200ms para o app processar o paste.
//!   7. Restaura o clipboard original.
//!
//! ## Por que o texto vai pro app "certo"
//!
//! O Whisper App nunca ganha foco durante o ciclo (a janela fica escondida
//! no tray, ou pelo menos não é ativada pelo hotkey). Então quando o usuário
//! solta F9, o Windows mantém o foco no app anterior — e é lá que o Ctrl+V
//! vai colar. Mas o pipeline (transcrição + LLM) leva alguns segundos, e
//! nesse meio tempo QUALQUER coisa pode roubar o foco — inclusive um clique
//! nos controles do overlay (`visual.rs`/`Overlay.tsx`). Por isso o chamador
//! pode passar o HWND capturado no momento do hotkey (`active_app::detect`)
//! e, no Windows, o restauramos como foreground logo antes do Ctrl+V.
//!
//! ## Limitações conhecidas
//!
//! - Se o usuário tinha uma imagem/arquivo no clipboard antes, o "restore"
//!   só recupera texto (arboard tem API limitada para outros tipos). Aceito
//!   no MVP.
//! - No Linux Wayland, isto pode falhar (Wayland restringe simulação de
//!   teclado). Precisa `ydotool` daemon ou portal XDG. PLATAFORMA: Wayland
//!   ainda não é suportado.

use std::thread;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use arboard::Clipboard;
use enigo::{Direction, Enigo, Key, Keyboard, Settings};

/// Cola o texto formatado no aplicativo ativo (via clipboard + Ctrl+V).
///
/// `target_hwnd` (Windows only, `None` nos outros SOs ou quando não há uma
/// janela capturada) é restaurado como foreground antes do Ctrl+V — ver
/// comentário do módulo. Passar `None` mantém o comportamento antigo: cola
/// em qualquer janela que já estiver em foco no momento da chamada.
///
/// Bloqueia por ~250ms (waits do clipboard/paste). Chamar em uma thread
/// dedicada (não bloqueia UI porque já é chamado do LlmService thread).
pub fn paste_text(text: &str, target_hwnd: Option<isize>) -> Result<()> {
    // 1. Salvar clipboard atual (só o texto — outros formatos ficam perdidos).
    let mut clipboard = Clipboard::new().context("falha ao acessar clipboard")?;
    let saved = clipboard.get_text().ok();

    // 2. Colocar o texto novo no clipboard.
    clipboard
        .set_text(text.to_string())
        .context("falha ao setar clipboard")?;

    // 3. Pequena espera pra garantir que o clipboard foi propagado antes
    //    do paste. Alguns apps demoram milissegundos para observar a mudança.
    thread::sleep(Duration::from_millis(50));

    // 4. Restaurar o foco na janela alvo, se houver uma (Windows only).
    #[cfg(target_os = "windows")]
    if let Some(hwnd) = target_hwnd {
        restore_foreground(hwnd);
    }
    #[cfg(not(target_os = "windows"))]
    let _ = target_hwnd;

    // 5. Simular Ctrl+V (Cmd+V no macOS).
    let mut enigo =
        Enigo::new(&Settings::default()).map_err(|e| anyhow!("falha ao criar enigo: {}", e))?;

    // PLATAFORMA: macOS usa Cmd, Windows/Linux usam Ctrl.
    let modifier = if cfg!(target_os = "macos") {
        Key::Meta
    } else {
        Key::Control
    };

    // Enigo retorna seu próprio tipo de erro. Convertemos para anyhow.
    enigo
        .key(modifier, Direction::Press)
        .map_err(|e| anyhow!("falha ao pressionar modificador: {}", e))?;
    enigo
        .key(Key::Unicode('v'), Direction::Click)
        .map_err(|e| anyhow!("falha ao pressionar V: {}", e))?;
    enigo
        .key(modifier, Direction::Release)
        .map_err(|e| anyhow!("falha ao soltar modificador: {}", e))?;

    // 6. Esperar o app-destino processar o paste antes de restaurar clipboard.
    //    200ms é suficiente para a maioria dos apps; excessivo se necessário
    //    prejudicará só experiência (não corretude).
    thread::sleep(Duration::from_millis(200));

    // 7. Restaurar clipboard original (se havia algo).
    //    `set_text` pode falhar se o clipboard estiver bloqueado por outro
    //    processo — ignoramos porque não é crítico (o pior que acontece é
    //    o usuário perder o clipboard antigo).
    if let Some(original) = saved {
        let _ = clipboard.set_text(original);
    }

    Ok(())
}

/// Traz `hwnd` de volta pro foreground, se ele ainda existir.
///
/// `SetForegroundWindow` sozinho costuma falhar quando o processo chamador
/// não é (e não acabou de ser) o processo em foreground — é uma restrição
/// deliberada do Windows contra apps em background "roubando" foco. O
/// workaround padrão é anexar temporariamente o input state da nossa thread
/// ao da thread dona da janela atualmente em foreground (`AttachThreadInput`),
/// o que concede a permissão; desanexamos logo em seguida. `IsWindow` evita
/// tentar restaurar uma janela que já foi fechada (o HWND pode até ter sido
/// reciclado para outra janela pelo Windows).
#[cfg(target_os = "windows")]
fn restore_foreground(hwnd: isize) {
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowThreadProcessId, IsWindow, SetForegroundWindow,
    };

    let target = hwnd as HWND;
    if target.is_null() || unsafe { IsWindow(target) } == 0 {
        return;
    }

    let foreground = unsafe { GetForegroundWindow() };
    if foreground == target {
        return; // já está lá, nada a fazer.
    }

    unsafe {
        let fg_tid = GetWindowThreadProcessId(foreground, std::ptr::null_mut());
        let our_tid = GetCurrentThreadId();

        let attached = fg_tid != 0 && fg_tid != our_tid && {
            AttachThreadInput(our_tid, fg_tid, 1) != 0
        };
        SetForegroundWindow(target);
        if attached {
            AttachThreadInput(our_tid, fg_tid, 0);
        }
    }
}
