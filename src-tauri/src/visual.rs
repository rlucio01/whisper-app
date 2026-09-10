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

use tauri::{AppHandle, LogicalSize, Manager, PhysicalPosition, Runtime};

use crate::config::{OverlayConfig, OverlayPosition, SharedConfig, VisualIndicator};

/// Label da janela flutuante (definido em `tauri.conf.json`).
const OVERLAY_LABEL: &str = "overlay";

/// Label do tray (definido em `lib.rs::build_tray`).
const TRAY_LABEL: &str = "main-tray";

/// Tamanho da barra em `scale: 1.0` (mesmos valores do `tauri.conf.json`).
/// `overlay.scale` multiplica isso pra dar zoom na janela nativa.
const BASE_WIDTH: f64 = 320.0;
const BASE_HEIGHT: f64 = 76.0;

/// Faixa de escala aceita — além disso a barra fica ilegível (muito pequena)
/// ou toma a tela inteira (muito grande) sem necessidade. Já testamos ir até
/// 25% (2026-08-19): mesmo com o conteúdo escalando proporcionalmente
/// (`Overlay.tsx` + `LogicalSize` aqui), ficou ilegível demais na prática —
/// voltamos pra 75% como piso.
const MIN_SCALE: f32 = 0.75;
const MAX_SCALE: f32 = 1.75;

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

fn current_overlay_config<R: Runtime>(app: &AppHandle<R>) -> OverlayConfig {
    match app.try_state::<SharedConfig>() {
        Some(state) => state
            .lock()
            .map(|g| g.overlay.clone())
            .unwrap_or_default(),
        None => OverlayConfig::default(),
    }
}

// ---------- Overlay window ----------

fn show_overlay<R: Runtime>(app: &AppHandle<R>) {
    let Some(window) = app.get_webview_window(OVERLAY_LABEL) else {
        return;
    };
    let cfg = current_overlay_config(app);
    apply_scale(&window, cfg.scale);
    position_overlay(&window, cfg.position, cfg.scale);
    // Reafirma a prioridade nativa toda vez que a barra reaparece.
    let _ = window.set_always_on_top(true);
    let _ = window.show();
    // Reafirma a posição logo após o show, garantindo que o compositor do Windows
    // aplique as coordenadas corretas mesmo no primeiro show da sessão.
    position_overlay(&window, cfg.position, cfg.scale);
    let _ = window.set_always_on_top(true);
}

fn hide_overlay<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window(OVERLAY_LABEL) {
        let _ = window.hide();
    }
}

/// Mostra uma prévia temporária do overlay por 3 segundos para teste visual.
pub fn preview_overlay<R: Runtime>(app: &AppHandle<R>) {
    show_overlay(app);
    let app_handle = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(3));
        if let Some(state) = app_handle.try_state::<crate::hotkey::SharedRecordingActive>() {
            if let Ok(active) = state.lock() {
                if *active {
                    return;
                }
            }
        }
        hide_overlay(&app_handle);
    });
}

/// Redimensiona a janela nativa do overlay conforme `overlay.scale` — é
/// assim que o "zoom" da barra é aplicado. `Overlay.tsx` renderiza o
/// conteúdo num box canônico de `BASE_WIDTH x BASE_HEIGHT` **pixels CSS** e
/// aplica o mesmo `scale` via `transform: scale()`; por isso o resize aqui
/// TEM que usar `LogicalSize` (pixels CSS/independentes de DPI), não
/// `PhysicalSize` (pixels de dispositivo) — num monitor com escala do
/// Windows diferente de 100% os dois divergem, e a janela real fica com um
/// tamanho CSS diferente do que o transform assume, cortando/desalinhando
/// o conteúdo.
fn apply_scale<R: Runtime>(window: &tauri::WebviewWindow<R>, scale: f32) {
    let scale = (scale as f64).clamp(MIN_SCALE as f64, MAX_SCALE as f64);
    let width = BASE_WIDTH * scale;
    let height = BASE_HEIGHT * scale;
    let _ = window.set_size(LogicalSize::new(width, height));
}

/// Resolve o monitor onde o overlay deve ser exibido.
///
/// No Windows, `window.current_monitor()` retorna `None` se a janela estiver
/// oculta (`visible: false`), o que impedia o posicionamento inicial em
/// notebooks e telas secundárias.
///
/// Esta função resolve com fallback robusto:
/// 1. Monitor onde está o cursor do mouse (onde o usuário está interagindo).
/// 2. Monitor atual da janela overlay (`current_monitor`).
/// 3. Monitor primário do sistema (`primary_monitor`).
/// 4. Primeiro monitor disponível (`available_monitors`).
fn resolve_target_monitor<R: Runtime>(window: &tauri::WebviewWindow<R>) -> Option<tauri::Monitor> {
    if let Ok(cursor_pos) = window.cursor_position() {
        if let Ok(monitors) = window.available_monitors() {
            let cx = cursor_pos.x as i32;
            let cy = cursor_pos.y as i32;
            for mon in monitors {
                let pos = mon.position();
                let size = mon.size();
                if cx >= pos.x
                    && cx < pos.x + size.width as i32
                    && cy >= pos.y
                    && cy < pos.y + size.height as i32
                {
                    return Some(mon);
                }
            }
        }
    }

    if let Ok(Some(mon)) = window.current_monitor() {
        return Some(mon);
    }

    if let Ok(Some(mon)) = window.primary_monitor() {
        return Some(mon);
    }

    window.available_monitors().ok().and_then(|m| m.into_iter().next())
}

/// Reposiciona o overlay no centro-horizontal, perto do topo ou do fundo
/// (conforme `overlay.position`) da tela em que a janela está.
fn position_overlay<R: Runtime>(
    window: &tauri::WebviewWindow<R>,
    position: OverlayPosition,
    scale: f32,
) {
    let Some(monitor) = resolve_target_monitor(window) else {
        return;
    };
    let mon_size = monitor.size();
    let mon_pos = monitor.position();
    let scale_factor = monitor.scale_factor();

    // Dimensão física calculada deterministicamente a partir da escala e DPI da tela.
    // Isso evita usar `outer_size()` quando a janela está oculta (onde pode retornar 0x0
    // no Windows, quebrando o cálculo de centralização em telas de notebooks).
    let scale = (scale as f64).clamp(MIN_SCALE as f64, MAX_SCALE as f64);
    let target_phys_w = (BASE_WIDTH * scale * scale_factor).round() as i32;
    let target_phys_h = (BASE_HEIGHT * scale * scale_factor).round() as i32;

    let (win_w, win_h) = match window.outer_size() {
        Ok(sz) if sz.width > 0 && sz.height > 0 => (sz.width as i32, sz.height as i32),
        _ => (target_phys_w, target_phys_h),
    };

    // Fração da altura do monitor onde fica o CENTRO vertical da barra.
    let y_fraction = match position {
        OverlayPosition::Bottom => 0.85,
        OverlayPosition::Top => 0.12,
    };

    let center_x = mon_pos.x + (mon_size.width as i32 - win_w) / 2;
    let center_y = mon_pos.y + (mon_size.height as f64 * y_fraction) as i32 - (win_h / 2);

    // Clamping para garantir que o overlay JAMAIS fique fora dos limites da tela
    // (ex: cortado pelo topo, pela lateral ou engolido pela barra de tarefas do Windows).
    let max_x = (mon_pos.x + mon_size.width as i32 - win_w).max(mon_pos.x);
    let max_y = (mon_pos.y + mon_size.height as i32 - win_h).max(mon_pos.y);

    let x = center_x.clamp(mon_pos.x, max_x);
    let y = center_y.clamp(mon_pos.y, max_y);

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
