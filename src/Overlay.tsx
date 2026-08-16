// Janela flutuante de indicação de estado — sempre no topo, transparente,
// aparece só durante o pipeline de gravação/transcrição/formatação.
//
// A visibilidade da JANELA é controlada pelo Rust (visual.rs — show/hide),
// mas o CONTEÚDO reage aos mesmos eventos que a UI principal escuta.

import { useEffect, useState } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import "./overlay.css";

type OverlayStatus = "recording" | "transcribing" | "formatting";

export default function Overlay() {
  const [status, setStatus] = useState<OverlayStatus>("recording");

  useEffect(() => {
    const unlistens: UnlistenFn[] = [];
    const track = (p: Promise<UnlistenFn>) => p.then((fn) => unlistens.push(fn));

    // Ao começar (F9 pressionado), volta pro estado inicial da barra.
    track(listen("hotkey-pressed", () => setStatus("recording")));
    track(
      listen<string>("transcription-status", (e) => {
        if (e.payload === "transcrevendo") setStatus("transcribing");
      })
    );
    track(listen("formatting-started", () => setStatus("formatting")));

    return () => unlistens.forEach((fn) => fn());
  }, []);

  const label = {
    recording: "Gravando",
    transcribing: "Transcrevendo",
    formatting: "Reformatando",
  }[status];

  return (
    <div className={`overlay-bar overlay-${status}`}>
      <div className="overlay-dot" />
      <span className="overlay-label">{label}</span>
    </div>
  );
}
