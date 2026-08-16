// Feedback sonoro do pipeline (start/end). Gera os tons via Web Audio API —
// zero assets, funciona mesmo com a janela principal escondida no tray,
// porque a webview continua viva.
//
// O Rust decide QUANDO tocar (respeita o toggle `config.sound_feedback`) —
// aqui só materializa os beeps.

let ctx: AudioContext | null = null;

function getCtx(): AudioContext | null {
  if (ctx) return ctx;
  try {
    // Alguns navegadores só permitem AudioContext depois de user gesture,
    // mas na webview do Tauri o global shortcut conta como um evento válido.
    // Se ainda assim falhar, silenciamos — o pipeline não deve quebrar por
    // causa de som.
    ctx = new AudioContext();
    return ctx;
  } catch {
    return null;
  }
}

export type SoundKind = "start" | "end";

/**
 * Toca um beep curto e suave. `start` = tom mais agudo (subindo),
 * `end` = tom mais grave (descendo). ~120ms cada, com fade in/out pra
 * não estalar.
 */
export function playBeep(kind: SoundKind) {
  const ac = getCtx();
  if (!ac) return;

  // Se o contexto estiver suspended (política de autoplay), tenta resumir.
  if (ac.state === "suspended") {
    ac.resume().catch(() => {});
  }

  const now = ac.currentTime;
  const duration = 0.12;

  // Frequências deliberadamente longe do lixo típico do sistema — tom limpo,
  // duas notas de piano bem consoantes.
  const [freqStart, freqEnd] =
    kind === "start" ? [660, 990] : [880, 494];

  const osc = ac.createOscillator();
  osc.type = "sine";
  osc.frequency.setValueAtTime(freqStart, now);
  osc.frequency.exponentialRampToValueAtTime(freqEnd, now + duration);

  const gain = ac.createGain();
  // Fade in/out pra evitar clique nas bordas.
  const peak = 0.15;
  gain.gain.setValueAtTime(0.0001, now);
  gain.gain.exponentialRampToValueAtTime(peak, now + 0.015);
  gain.gain.exponentialRampToValueAtTime(0.0001, now + duration);

  osc.connect(gain).connect(ac.destination);
  osc.start(now);
  osc.stop(now + duration + 0.02);
}
