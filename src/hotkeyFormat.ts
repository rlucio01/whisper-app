export const DEFAULT_HOTKEY = "Ctrl+Super";
export const DEFAULT_TRANSLATE_TOGGLE_HOTKEY = "F10";

export const MOUSE_HOTKEY_PRESETS = [
  { id: "Mouse4", label: "Mouse 4 (Voltar)", desc: "Botão lateral inferior do mouse" },
  { id: "Mouse5", label: "Mouse 5 (Avançar)", desc: "Botão lateral superior do mouse" },
  { id: "MiddleClick", label: "Botão do Meio", desc: "Clique da rodinha (Scroll)" },
];

/** "Super" é o nome que o backend entende pra tecla Windows, mas o usuário
 *  pensa nela como "Windows" — só troca pra exibição, o valor salvo continua
 *  em "Super". Também traduz botões de mouse para nomes amigáveis. */
export function displayHotkey(value: string): string {
  if (!value) return "";
  const v = value.trim().toLowerCase();
  if (v === "mouse4" || v === "xbutton1" || v === "mouseback") return "Mouse 4 (Lateral Voltar)";
  if (v === "mouse5" || v === "xbutton2" || v === "mouseforward") return "Mouse 5 (Lateral Avançar)";
  if (v === "middleclick" || v === "middle" || v === "mouse3") return "Botão do Meio (Scroll)";
  return value.replace(/\bSuper\b/g, "Windows");
}

/** Mapeia o `event.code` de cada tecla modificadora (esquerda/direita) para
 *  o nome canônico usado nas strings de atalho. */
export const MOD_CODE_TO_NAME: Record<string, string> = {
  ControlLeft: "Ctrl",
  ControlRight: "Ctrl",
  ShiftLeft: "Shift",
  ShiftRight: "Shift",
  AltLeft: "Alt",
  AltRight: "Alt",
  MetaLeft: "Super",
  MetaRight: "Super",
  OSLeft: "Super",
  OSRight: "Super",
};

export const MOD_ORDER = ["Ctrl", "Shift", "Alt", "Super"];

/** Monta a string de uma combinação só de modificadores (ex: "Ctrl+Super"
 *  para "Ctrl+Windows"), na ordem canônica. */
export function formatModifierCombo(mods: Set<string>): string {
  return MOD_ORDER.filter((m) => mods.has(m)).join("+");
}

/** Mapeia `event.code` (código físico da tecla, independente de layout) para
 *  o nome que o parser de accelerator do Tauri aceita. */
export function codeToShortcutKey(code: string): string | null {
  if (/^Key[A-Z]$/.test(code)) return code.slice(3);
  if (/^Digit\d$/.test(code)) return code.slice(5);
  if (/^F\d{1,2}$/.test(code)) return code;
  if (/^Numpad\d$/.test(code)) return "Num" + code.slice(6);

  const map: Record<string, string> = {
    Space: "Space",
    Enter: "Enter",
    NumpadEnter: "Enter",
    Escape: "Escape",
    Backspace: "Backspace",
    Tab: "Tab",
    CapsLock: "CapsLock",
    Insert: "Insert",
    Delete: "Delete",
    Home: "Home",
    End: "End",
    PageUp: "PageUp",
    PageDown: "PageDown",
    ArrowUp: "Up",
    ArrowDown: "Down",
    ArrowLeft: "Left",
    ArrowRight: "Right",
    Minus: "-",
    Equal: "=",
    Comma: ",",
    Period: ".",
    Slash: "/",
    Backslash: "\\",
    Semicolon: ";",
    Quote: "'",
    BracketLeft: "[",
    BracketRight: "]",
    Backquote: "`",
    PrintScreen: "PrintScreen",
    ScrollLock: "ScrollLock",
    Pause: "Pause",
  };
  return map[code] ?? null;
}

/** Converte um KeyboardEvent numa string aceita pelo `Shortcut::from_str`.
 *  Retorna null se for só um modificador (ex: só Shift). */
export function formatHotkey(e: KeyboardEvent): string | null {
  if (MOD_CODE_TO_NAME[e.code]) return null;

  const key = codeToShortcutKey(e.code);
  if (!key) return null;

  const parts: string[] = [];
  if (e.ctrlKey) parts.push("Ctrl");
  if (e.shiftKey) parts.push("Shift");
  if (e.altKey) parts.push("Alt");
  if (e.metaKey) parts.push("Super");
  parts.push(key);
  return parts.join("+");
}

/** Verifica se um evento de teclado bate com o atalho alvo configurado. */
export function isHotkeyMatch(
  e: KeyboardEvent,
  target: string,
  heldMods?: Set<string>,
): boolean {
  if (!target || !target.trim()) return false;
  const targetNorm = target.trim().toLowerCase();

  // Combinação apenas de modificadores (ex: Ctrl+Super)
  if (heldMods && heldMods.size >= 2) {
    const combo = formatModifierCombo(heldMods).toLowerCase();
    if (combo === targetNorm) return true;
  }

  // Tecla com modificadores
  const formatted = formatHotkey(e)?.toLowerCase();
  if (formatted && formatted === targetNorm) return true;

  // Tecla simples (ex: F9 ou F10 sem modificadores)
  const single = codeToShortcutKey(e.code)?.toLowerCase();
  if (
    single &&
    single === targetNorm &&
    !e.ctrlKey &&
    !e.altKey &&
    !e.metaKey &&
    !e.shiftKey
  ) {
    return true;
  }

  return false;
}
