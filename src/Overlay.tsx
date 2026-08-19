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

import { useEffect, useRef, useState, type CSSProperties, type ReactElement } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import "./overlay.css";

type OverlayStatus = "recording" | "transcribing" | "formatting";

// Espelha `OverlayConfig` em config.rs. `position` não é usado aqui — quem
// decide onde a janela fica é o Rust (`visual::position_overlay`); só
// `opacity`/`accent_color` viram CSS custom properties no elemento raiz.
interface OverlayConfig {
  position: "top" | "bottom";
  scale: number;
  opacity: number;
  accent_color: string;
}

const DEFAULT_OVERLAY_CONFIG: OverlayConfig = {
  position: "bottom",
  scale: 1,
  opacity: 0.92,
  accent_color: "#ef4444",
};

// Precisa espelhar `BASE_WIDTH`/`BASE_HEIGHT` em visual.rs. O Rust
// redimensiona a janela nativa pra `BASE * scale`; aqui renderizamos o
// conteúdo sempre no tamanho canônico e aplicamos o mesmo `scale` via CSS
// transform. Sem isso, em escalas pequenas (ex: 25%) as margens/paddings em
// px fixos da barra não encolhem junto com a janela e o conteúdo fica
// cortado/ilegível — com o transform, tudo (barras, texto, botões) escala
// proporcionalmente, exatamente como o app inteiro tivesse dado zoom.
const BASE_WIDTH = 320;
const BASE_HEIGHT = 76;

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
  const [overlayConfig, setOverlayConfig] = useState<OverlayConfig>(DEFAULT_OVERLAY_CONFIG);
  const intervalRef = useRef<number | null>(null);

  useEffect(() => {
    invoke<{ overlay: OverlayConfig }>("get_config")
      .then((cfg) => setOverlayConfig(cfg.overlay))
      .catch((e) => console.error("get_config falhou:", e));

    // A janela do overlay é criada uma vez no boot e só escondida/mostrada
    // depois — sem isso, mudar opacidade/cor em settings só valeria depois
    // de reiniciar o app inteiro.
    const unlisten = listen<OverlayConfig>("config-updated", (e) => {
      setOverlayConfig(e.payload);
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

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

  // Custom properties lidas por overlay.css (`--overlay-opacity`,
  // `--overlay-accent`) — o cast é porque o React não tipa CSS vars nativamente.
  const barStyle = {
    "--overlay-opacity": overlayConfig.opacity,
    "--overlay-accent": overlayConfig.accent_color,
  } as CSSProperties;

  let content: ReactElement;
  if (status === "recording") {
    content = (
      <div
        className="overlay-bar overlay-recording"
        style={barStyle}
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
  } else {
    const label = { transcribing: "Transcrevendo", formatting: "Reformatando" }[status];
    content = (
      <div className={`overlay-bar overlay-${status}`} style={barStyle}>
        <div className="overlay-dot" />
        <span className="overlay-label">{label}</span>
      </div>
    );
  }

  return (
    <div
      className="overlay-scale-wrapper"
      style={{
        width: BASE_WIDTH,
        height: BASE_HEIGHT,
        transform: `scale(${overlayConfig.scale})`,
        transformOrigin: "top left",
      }}
    >
      {content}
    </div>
  );
}
