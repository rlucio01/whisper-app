//! Feedback sonoro do pipeline. Emite um evento Tauri (`play-sound`) para o
//! frontend, que gera o som via Web Audio API. Fazer no lado JS evita puxar
//! um crate de áudio (`rodio`) só pra isso — a webview do main window fica
//! viva mesmo quando escondida no tray, então os beeps continuam tocando.
//!
//! Respeita o toggle `config.sound_feedback` — se estiver desligado, nem
//! emitimos o evento (nada chega no front, nada toca).

use tauri::{AppHandle, Emitter, Manager, Runtime};

use crate::config::SharedConfig;

/// Marca qual momento do pipeline disparou o som — o frontend decide o tom.
#[derive(Debug, Clone, Copy)]
pub enum Kind {
    /// Começou a gravar (F9 pressionado).
    Start,
    /// Pipeline terminou (texto colado com sucesso).
    End,
}

impl Kind {
    fn tag(self) -> &'static str {
        match self {
            Kind::Start => "start",
            Kind::End => "end",
        }
    }
}

/// Emite o evento se o usuário tiver o feedback sonoro ativado.
/// Se o config estiver indisponível por algum motivo, cai no default (toca).
pub fn play<R: Runtime>(app: &AppHandle<R>, kind: Kind) {
    let enabled = app
        .try_state::<SharedConfig>()
        .and_then(|s| s.lock().ok().map(|g| g.sound_feedback))
        .unwrap_or(true);
    if !enabled {
        return;
    }
    let _ = app.emit("play-sound", kind.tag());
}
