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
//!   4. Simula Ctrl+V (Cmd+V no macOS).
//!   5. Espera ~200ms para o app processar o paste.
//!   6. Restaura o clipboard original.
//!
//! ## Por que o texto vai pro app "certo"
//!
//! O Whisper App nunca ganha foco durante o ciclo (a janela fica escondida
//! no tray, ou pelo menos não é ativada pelo hotkey). Então quando o usuário
//! solta F9, o Windows mantém o foco no app anterior — e é lá que o Ctrl+V
//! vai colar.
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
/// Bloqueia por ~250ms (waits do clipboard/paste). Chamar em uma thread
/// dedicada (não bloqueia UI porque já é chamado do LlmService thread).
pub fn paste_text(text: &str) -> Result<()> {
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

    // 4. Simular Ctrl+V (Cmd+V no macOS).
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

    // 5. Esperar o app-destino processar o paste antes de restaurar clipboard.
    //    200ms é suficiente para a maioria dos apps; excessivo se necessário
    //    prejudicará só experiência (não corretude).
    thread::sleep(Duration::from_millis(200));

    // 6. Restaurar clipboard original (se havia algo).
    //    `set_text` pode falhar se o clipboard estiver bloqueado por outro
    //    processo — ignoramos porque não é crítico (o pior que acontece é
    //    o usuário perder o clipboard antigo).
    if let Some(original) = saved {
        let _ = clipboard.set_text(original);
    }

    Ok(())
}
