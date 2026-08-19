// Janela flutuante de indicação de estado — sempre no topo, transparente,
// aparece só durante o pipeline de gravação/transcrição/formatação.
//
// A visibilidade da JANELA é controlada pelo Rust (visual.rs — show/hide),
// mas o CONTEÚDO reage aos mesmos eventos que a UI principal escuta.
//
// Durante a gravação, a barra mostra uma onda que acompanha o volume do
// microfone (evento `audio-level`, emitido por `audio.rs` a ~30/s). Passar
// o mouse por cima troca a onda por controles rápidos: cancelar (✕), tempo
// decorrido e concluir agora (✓) — ver `commands::cancel_recording` /
// `commands::confirm_recording`.

import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import "./overlay.css";

type OverlayStatus = "recording" | "transcribing" | "formatting";

const BAR_COUNT = 28;

function emptyBars(): number[] {
  return new Array(BAR_COUNT).fill(0);
}

function formatElapsed(totalSeconds: number): string {
  const m = Math.floor(totalSeconds / 60);
  const s = totalSeconds % 60;
  return `${m}:${String(s).padStart(2, "0")}`;
}

export default function Overlay() {
  const [status, setStatus] = useState<OverlayStatus>("recording");
  const [bars, setBars] = useState<number[]>(emptyBars);
  const [elapsed, setElapsed] = useState(0);
  const [hovered, setHovered] = useState(false);
  const intervalRef = useRef<number | null>(null);

  useEffect(() => {
    const stopTimer = () => {
      if (intervalRef.current !== null) {
        window.clearInterval(intervalRef.current);
        intervalRef.current = null;
      }
    };

    const unlistens: UnlistenFn[] = [];
    const track = (p: Promise<UnlistenFn>) => p.then((fn) => unlistens.push(fn));

    // Ao começar (F9 pressionado), volta pro estado inicial da barra e
    // reinicia o cronômetro — independente do que estava rolando antes
    // (cobre o caso de cancelar e começar de novo rapidinho).
    track(
      listen("hotkey-pressed", () => {
        setStatus("recording");
        setBars(emptyBars());
        setElapsed(0);
        stopTimer();
        const start = Date.now();
        intervalRef.current = window.setInterval(() => {
          setElapsed(Math.floor((Date.now() - start) / 1000));
        }, 250);
      }),
    );

    track(
      listen<number>("audio-level", (e) => {
        setBars((prev) => [...prev.slice(1), Math.max(0, Math.min(1, e.payload))]);
      }),
    );

    track(
      listen("recording-cancelled", () => {
        stopTimer();
        setBars(emptyBars());
        setElapsed(0);
      }),
    );

    track(
      listen<string>("transcription-status", (e) => {
        if (e.payload === "transcrevendo") {
          setStatus("transcribing");
          stopTimer();
        }
      }),
    );
    track(
      listen("formatting-started", () => {
        setStatus("formatting");
        stopTimer();
      }),
    );

    return () => {
      stopTimer();
      unlistens.forEach((fn) => fn());
    };
  }, []);

  const handleCancel = () => {
    invoke("cancel_recording").catch((e) => console.error("cancel_recording falhou:", e));
  };
  const handleConfirm = () => {
    invoke("confirm_recording").catch((e) => console.error("confirm_recording falhou:", e));
  };

  if (status === "recording") {
    return (
      <div
        className="overlay-bar overlay-recording"
        onMouseEnter={() => setHovered(true)}
        onMouseLeave={() => setHovered(false)}
      >
        {hovered ? (
          <div className="overlay-controls">
            <button
              className="overlay-btn overlay-btn-cancel"
              onClick={handleCancel}
              title="Cancelar gravação"
              aria-label="Cancelar gravação"
            >
              ✕
            </button>
            <span className="overlay-timer">{formatElapsed(elapsed)}</span>
            <button
              className="overlay-btn overlay-btn-confirm"
              onClick={handleConfirm}
              title="Concluir agora"
              aria-label="Concluir agora"
            >
              ✓
            </button>
          </div>
        ) : (
          <div className="overlay-waveform">
            {bars.map((v, i) => (
              <span
                key={i}
                className="overlay-wave-bar"
                style={{ height: `${8 + v * 92}%` }}
              />
            ))}
          </div>
        )}
      </div>
    );
  }

  const label = { transcribing: "Transcrevendo", formatting: "Reformatando" }[status];

  return (
    <div className={`overlay-bar overlay-${status}`}>
      <div className="overlay-dot" />
      <span className="overlay-label">{label}</span>
    </div>
  );
}
