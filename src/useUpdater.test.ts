import { describe, it, expect, vi, beforeEach } from "vitest";
import { renderHook, act } from "@testing-library/react";
import { useUpdater } from "./useUpdater";
import { check } from "@tauri-apps/plugin-updater";
import { invoke } from "@tauri-apps/api/core";

vi.mock("@tauri-apps/plugin-updater", () => ({
  check: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn().mockResolvedValue(null),
}));

describe("useUpdater", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("should initialize with idle status", () => {
    const { result } = renderHook(() => useUpdater());
    expect(result.current.info.status).toBe("idle");
    expect(result.current.info.version).toBeNull();
    expect(result.current.info.error).toBeNull();
  });

  it("should handle up_to_date when no update is found", async () => {
    vi.mocked(check).mockResolvedValueOnce(null);

    const { result } = renderHook(() => useUpdater());

    await act(async () => {
      await result.current.checkNow();
    });

    expect(result.current.info.status).toBe("up_to_date");
    expect(invoke).toHaveBeenCalledWith("set_tray_update_available", { version: null });
  });

  it("should handle available update when new version exists", async () => {
    const mockUpdate = {
      version: "1.2.0",
      body: "Melhorias de desempenho",
      downloadAndInstall: vi.fn(),
    };
    // @ts-expect-error mock partial Update
    vi.mocked(check).mockResolvedValueOnce(mockUpdate);

    const { result } = renderHook(() => useUpdater());

    await act(async () => {
      await result.current.checkNow();
    });

    expect(result.current.info.status).toBe("available");
    expect(result.current.info.version).toBe("1.2.0");
    expect(result.current.info.notes).toBe("Melhorias de desempenho");
    expect(invoke).toHaveBeenCalledWith("set_tray_update_available", { version: "1.2.0" });
  });

  it("should set error status on check failure if not silent", async () => {
    vi.mocked(check).mockRejectedValueOnce(new Error("Network offline"));

    const { result } = renderHook(() => useUpdater());

    await act(async () => {
      await result.current.checkNow({ silent: false });
    });

    expect(result.current.info.status).toBe("error");
    expect(result.current.info.error).toContain("Network offline");
  });

  it("should remain silent on check failure when silent: true", async () => {
    vi.mocked(check).mockRejectedValueOnce(new Error("Network offline"));

    const { result } = renderHook(() => useUpdater());

    await act(async () => {
      await result.current.checkNow({ silent: true });
    });

    expect(result.current.info.status).toBe("idle");
    expect(result.current.info.error).toBeNull();
  });
});
