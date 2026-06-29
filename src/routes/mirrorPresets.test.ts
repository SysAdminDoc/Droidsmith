import { describe, expect, it } from "vitest";

import {
  DEFAULT_MIRROR_PRESET,
  normalizePreset,
  presetStorageKey,
} from "./mirrorPresets";

describe("mirror presets", () => {
  it("fills missing fields from the default preset", () => {
    expect(normalizePreset({ bitRate: "16M", noAudio: true })).toEqual({
      ...DEFAULT_MIRROR_PRESET,
      bitRate: "16M",
      noAudio: true,
    });
  });

  it("rejects unknown keyboard modes while preserving valid ones", () => {
    expect(normalizePreset({ keyboardMode: "uhid" }).keyboardMode).toBe("uhid");
    expect(normalizePreset({ keyboardMode: "bad" as never }).keyboardMode).toBe(
      "default",
    );
  });

  it("scopes saved presets to each device serial", () => {
    expect(presetStorageKey("DEVICE123")).toBe(
      "droidsmith.mirror.preset.DEVICE123",
    );
  });
});
