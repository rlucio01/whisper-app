//! Indicadores visuais do estado atual do app (gravando / transcrevendo /
//! reformatando). Combinam:
//!
//! - Uma **janela flutuante** ("overlay") sempre-no-topo, sem decoração,
//!   posicionada no centro-inferior da tela. Aparece só durante o pipeline.
//! - O **ícone da bandeja** trocando pra vermelho enquanto grava.
//!
//! O usuário escolhe entre floating / tray / both / none via config
//! (campo `visual_indicator`).
//!
//! ## Por que dois indicadores?
//!
//! Quando o Whisper App está no tray e outros apps cobrem a janela principal,
//! não dá pra saber se a gravação começou. A janela flutuante resolve isso —
//! ela aparece por cima de qualquer coisa (`alwaysOnTop`), mas sem roubar
//! foco (`focus: false` no tauri.conf.json), pra não interferir no app onde
//! o texto vai ser colado depois.

use tauri::{AppHandle, Manager, PhysicalPosition, Runtime};

use crate::config::{SharedConfig, VisualIndicator};

/// Label da janela flutuante (definido em `tauri.conf.json`).
const OVERLAY_LABEL: &str = "overlay";

/// Label do tray (definido em `lib.rs::build_tray`).
const TRAY_LABEL: &str = "main-tray";

/// Estados sinalizados pelo indicador. O texto/cor no overlay é decidido
/// no lado JS a partir dos eventos que já emitimos (hotkey-pressed, etc.).
/// Aqui só nos importa "mostrar/esconder" o overlay e trocar ou não o tray.
#[derive(Clone, Copy)]
pub enum State {
    /// Gravação começou (F9 pressionado).
    Recording,
    /// Pipeline terminou (com sucesso ou erro) — esconder tudo.
    Idle,
}

/// Aplica o estado visual respeitando a config atual do usuário.
pub fn set<R: Runtime>(app: &AppHandle<R>, state: State) {
    let indicator = current_indicator(app);

    match state {
        State::Recording => {
            if indicator.uses_floating() {
                show_overlay(app);
            }
            if indicator.uses_tray() {
                set_tray_recording(app);
            }
        }
        State::Idle => {
            // Independente do config, sempre garantimos que o overlay não fica
            // preso na tela. Isso é seguro — se já estava escondido, no-op.
            hide_overlay(app);
            set_tray_normal(app);
        }
    }
}

fn current_indicator<R: Runtime>(app: &AppHandle<R>) -> VisualIndicator {
    match app.try_state::<SharedConfig>() {
        Some(state) => state.lock().map(|g| g.visual_indicator).unwrap_or_default(),
        None => VisualIndicator::default(),
    }
}

// ---------- Overlay window ----------

fn show_overlay<R: Runtime>(app: &AppHandle<R>) {
    let Some(window) = app.get_webview_window(OVERLAY_LABEL) else {
        return;
    };
    position_overlay(&window);
    let _ = window.show();
}

fn hide_overlay<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window(OVERLAY_LABEL) {
        let _ = window.hide();
    }
}

/// Reposiciona o overlay no centro-horizontal, um pouco acima do fundo da
/// tela em que o mouse (ou a janela principal) está.
fn position_overlay<R: Runtime>(window: &tauri::WebviewWindow<R>) {
    let Ok(Some(monitor)) = window.current_monitor() else {
        return;
    };
    let mon_size = monitor.size();
    let mon_pos = monitor.position();
    let win_size = window.outer_size().unwrap_or_default();

    // Centro horizontal, 85% da altura (barra na parte inferior).
    let x = mon_pos.x + (mon_size.width as i32 - win_size.width as i32) / 2;
    let y = mon_pos.y + (mon_size.height as f64 * 0.85) as i32
        - (win_size.height as i32 / 2);

    let _ = window.set_position(PhysicalPosition { x, y });
}

// ---------- Tray icon ----------

fn set_tray_recording<R: Runtime>(app: &AppHandle<R>) {
    if let Some(tray) = app.tray_by_id(TRAY_LABEL) {
        let _ = tray.set_icon(Some(make_recording_icon()));
    }
}

fn set_tray_normal<R: Runtime>(app: &AppHandle<R>) {
    if let Some(tray) = app.tray_by_id(TRAY_LABEL) {
        // Volta pro ícone default do app (mesmo que a janela principal).
        if let Some(default_icon) = app.default_window_icon().cloned() {
            let _ = tray.set_icon(Some(default_icon));
        }
    }
}

/// Gera um ícone "gravando" — círculo vermelho preenchido em fundo transparente.
/// 32x32 é o tamanho padrão de tray no Windows.
fn make_recording_icon() -> tauri::image::Image<'static> {
    const SIZE: u32 = 32;
    let mut bytes = vec![0u8; (SIZE * SIZE * 4) as usize];

    let cx = SIZE as f32 / 2.0;
    let cy = SIZE as f32 / 2.0;
    let radius = SIZE as f32 * 0.42;

    for y in 0..SIZE {
        for x in 0..SIZE {
            let dx = x as f32 - cx;
            let dy = y as f32 - cy;
            let dist = (dx * dx + dy * dy).sqrt();
            let idx = ((y * SIZE + x) * 4) as usize;

            if dist <= radius {
                // #ef4444 (mesmo vermelho do CSS)
                bytes[idx] = 239;
                bytes[idx + 1] = 68;
                bytes[idx + 2] = 68;
                bytes[idx + 3] = 255;
            } else if dist <= radius + 1.0 {
                // Antialias leve na borda pra não ficar serrilhado.
                let alpha = ((radius + 1.0 - dist) * 255.0) as u8;
                bytes[idx] = 239;
                bytes[idx + 1] = 68;
                bytes[idx + 2] = 68;
                bytes[idx + 3] = alpha;
            }
            // fora do círculo: fica transparente (todos zeros)
        }
    }

    tauri::image::Image::new_owned(bytes, SIZE, SIZE)
}
