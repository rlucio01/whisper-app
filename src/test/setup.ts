import "@testing-library/jest-dom/vitest";
import { vi } from "vitest";

// Mock para APIs nativas do Tauri 2 para que componentes possam renderizar em ambiente de teste
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn().mockImplementation(async (cmd: string) => {
    if (cmd === "get_config") {
      return {
        provider: "openai",
        openai_api_key: "",
        anthropic_api_key: "",
        openrouter_api_key: "",
        groq_api_key: "",
        gemini_api_key: "",
        xai_api_key: "",
        llm_model: "gpt-4o-mini",
        custom_words: [],
        replacements: [],
        device_selection: "auto",
        translate: { enabled: false, target_language: "en" },
        visual_indicator: "both",
        hotkey: "Ctrl+Super",
        secondary_hotkey: "",
        translate_toggle_hotkey: "F10",
        hands_free_hotkey: "",
        repaste_hotkey: "",
        transcription_provider: "local",
        whisper_model: "small",
        microphone: "",
        start_minimized: false,
        mute_system_audio: false,
        dictionary: { enabled: true, custom_words: [], replacements: [] },
      };
    }
    return null;
  }),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
  emit: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("@tauri-apps/plugin-updater", () => ({
  check: vi.fn().mockResolvedValue(null),
}));

vi.mock("@tauri-apps/plugin-process", () => ({
  relaunch: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("@tauri-apps/plugin-opener", () => ({
  openUrl: vi.fn().mockResolvedValue(undefined),
}));
