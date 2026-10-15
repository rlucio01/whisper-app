import { describe, it, expect, vi, beforeEach } from "vitest";
import { playBeep } from "./sound";

describe("sound", () => {
  const mockOscillator = {
    type: "",
    frequency: {
      setValueAtTime: vi.fn(),
      exponentialRampToValueAtTime: vi.fn(),
    },
    connect: vi.fn().mockReturnThis(),
    start: vi.fn(),
    stop: vi.fn(),
  };

  const mockGain = {
    gain: {
      setValueAtTime: vi.fn(),
      exponentialRampToValueAtTime: vi.fn(),
    },
    connect: vi.fn().mockReturnThis(),
  };

  beforeEach(() => {
    vi.clearAllMocks();

    class MockAudioContext {
      currentTime = 10;
      state = "running";
      resume = vi.fn().mockResolvedValue(undefined);
      createOscillator = vi.fn().mockReturnValue(mockOscillator);
      createGain = vi.fn().mockReturnValue(mockGain);
      destination = {};
    }

    // @ts-expect-error mock global AudioContext
    window.AudioContext = MockAudioContext;
  });

  it("should play start beep with ascending frequency", () => {
    playBeep("start");
    expect(mockOscillator.frequency.setValueAtTime).toHaveBeenCalledWith(660, 10);
    expect(mockOscillator.frequency.exponentialRampToValueAtTime).toHaveBeenCalledWith(990, 10 + 0.12);
    expect(mockOscillator.start).toHaveBeenCalledWith(10);
  });

  it("should play end beep with descending frequency", () => {
    playBeep("end");
    expect(mockOscillator.frequency.setValueAtTime).toHaveBeenCalledWith(880, 10);
    expect(mockOscillator.frequency.exponentialRampToValueAtTime).toHaveBeenCalledWith(494, 10 + 0.12);
    expect(mockOscillator.start).toHaveBeenCalledWith(10);
  });
});
