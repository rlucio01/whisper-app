// Espelha `hotkey::DEFAULT_HOTKEY` em `src-tauri/src/hotkey.rs` — usado
// quando o campo `hotkey` do config está vazio.
export const DEFAULT_HOTKEY = "Ctrl+Super";

/** "Super" é o nome que o backend entende pra tecla Windows, mas o usuário
 *  pensa nela como "Windows" — só troca pra exibição, o valor salvo continua
 *  em "Super". */
export function displayHotkey(value: string): string {
  return value.replace(/\bSuper\b/g, "Windows");
}
