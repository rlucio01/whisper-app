import { describe, it, expect } from "vitest";
import {
  displayHotkey,
  formatModifierCombo,
  codeToShortcutKey,
  formatHotkey,
  isHotkeyMatch,
  DEFAULT_HOTKEY,
  DEFAULT_TRANSLATE_TOGGLE_HOTKEY,
  MOUSE_HOTKEY_PRESETS,
} from "./hotkeyFormat";

describe("hotkeyFormat", () => {
  describe("constants", () => {
    it("should have correct defaults", () => {
      expect(DEFAULT_HOTKEY).toBe("Ctrl+Super");
      expect(DEFAULT_TRANSLATE_TOGGLE_HOTKEY).toBe("F10");
      expect(MOUSE_HOTKEY_PRESETS).toHaveLength(3);
    });
  });

  describe("displayHotkey", () => {
    it("should replace 'Super' with 'Windows'", () => {
      expect(displayHotkey("Ctrl+Super")).toBe("Ctrl+Windows");
      expect(displayHotkey("Super+Alt")).toBe("Windows+Alt");
    });

    it("should display user-friendly names for mouse buttons", () => {
      expect(displayHotkey("Mouse4")).toBe("Mouse 4 (Lateral Voltar)");
      expect(displayHotkey("xbutton1")).toBe("Mouse 4 (Lateral Voltar)");
      expect(displayHotkey("Mouse5")).toBe("Mouse 5 (Lateral Avançar)");
      expect(displayHotkey("xbutton2")).toBe("Mouse 5 (Lateral Avançar)");
      expect(displayHotkey("MiddleClick")).toBe("Botão do Meio (Scroll)");
      expect(displayHotkey("middle")).toBe("Botão do Meio (Scroll)");
    });

    it("should handle empty or standard values safely", () => {
      expect(displayHotkey("")).toBe("");
      expect(displayHotkey("Ctrl+Shift+A")).toBe("Ctrl+Shift+A");
      expect(displayHotkey("F10")).toBe("F10");
    });
  });

  describe("formatModifierCombo", () => {
    it("should order modifiers according to MOD_ORDER (Ctrl, Shift, Alt, Super)", () => {
      const set1 = new Set(["Super", "Ctrl"]);
      expect(formatModifierCombo(set1)).toBe("Ctrl+Super");

      const set2 = new Set(["Alt", "Ctrl", "Shift", "Super"]);
      expect(formatModifierCombo(set2)).toBe("Ctrl+Shift+Alt+Super");
    });

    it("should return empty string for empty set", () => {
      expect(formatModifierCombo(new Set())).toBe("");
    });
  });

  describe("codeToShortcutKey", () => {
    it("should convert letter keys", () => {
      expect(codeToShortcutKey("KeyA")).toBe("A");
      expect(codeToShortcutKey("KeyZ")).toBe("Z");
    });

    it("should convert digit keys", () => {
      expect(codeToShortcutKey("Digit0")).toBe("0");
      expect(codeToShortcutKey("Digit9")).toBe("9");
    });

    it("should convert function keys", () => {
      expect(codeToShortcutKey("F1")).toBe("F1");
      expect(codeToShortcutKey("F10")).toBe("F10");
      expect(codeToShortcutKey("F12")).toBe("F12");
    });

    it("should convert numpad keys", () => {
      expect(codeToShortcutKey("Numpad0")).toBe("Num0");
      expect(codeToShortcutKey("Numpad9")).toBe("Num9");
    });

    it("should convert special keys", () => {
      expect(codeToShortcutKey("Space")).toBe("Space");
      expect(codeToShortcutKey("Enter")).toBe("Enter");
      expect(codeToShortcutKey("Escape")).toBe("Escape");
      expect(codeToShortcutKey("ArrowUp")).toBe("Up");
      expect(codeToShortcutKey("ArrowDown")).toBe("Down");
    });

    it("should return null for unrecognized keys", () => {
      expect(codeToShortcutKey("UnrecognizedKey")).toBeNull();
    });
  });

  describe("formatHotkey", () => {
    it("should return null if code is a pure modifier key", () => {
      const event = { code: "ControlLeft" } as KeyboardEvent;
      expect(formatHotkey(event)).toBeNull();
    });

    it("should format single key with modifiers", () => {
      const event = {
        code: "KeyC",
        ctrlKey: true,
        shiftKey: false,
        altKey: false,
        metaKey: false,
      } as KeyboardEvent;
      expect(formatHotkey(event)).toBe("Ctrl+C");
    });

    it("should format multiple modifiers with key", () => {
      const event = {
        code: "KeyV",
        ctrlKey: true,
        shiftKey: true,
        altKey: false,
        metaKey: true,
      } as KeyboardEvent;
      expect(formatHotkey(event)).toBe("Ctrl+Shift+Super+V");
    });
  });

  describe("isHotkeyMatch", () => {
    it("should match modifier combos using heldMods", () => {
      const heldMods = new Set(["Ctrl", "Super"]);
      const event = { code: "ControlLeft" } as KeyboardEvent;
      expect(isHotkeyMatch(event, "Ctrl+Super", heldMods)).toBe(true);
      expect(isHotkeyMatch(event, "Ctrl+Shift", heldMods)).toBe(false);
    });

    it("should match single function keys without modifiers", () => {
      const event = {
        code: "F10",
        ctrlKey: false,
        altKey: false,
        metaKey: false,
        shiftKey: false,
      } as KeyboardEvent;
      expect(isHotkeyMatch(event, "F10")).toBe(true);
      expect(isHotkeyMatch(event, "F9")).toBe(false);
    });

    it("should not match single function keys if unexpected modifiers are held", () => {
      const event = {
        code: "F10",
        ctrlKey: true,
        altKey: false,
        metaKey: false,
        shiftKey: false,
      } as KeyboardEvent;
      expect(isHotkeyMatch(event, "F10")).toBe(false);
    });

    it("should match combination with modifiers and key", () => {
      const event = {
        code: "KeyS",
        ctrlKey: true,
        altKey: false,
        metaKey: false,
        shiftKey: false,
      } as KeyboardEvent;
      expect(isHotkeyMatch(event, "Ctrl+S")).toBe(true);
      expect(isHotkeyMatch(event, "Ctrl+A")).toBe(false);
    });

    it("should return false for empty target", () => {
      const event = { code: "KeyA" } as KeyboardEvent;
      expect(isHotkeyMatch(event, "")).toBe(false);
      expect(isHotkeyMatch(event, "   ")).toBe(false);
    });
  });
});
