import { describe, expect, it } from "vitest";

import {
  DEFAULT_MIRROR_PRESET,
  normalizePreset,
  presetStorageKey,
  type MirrorPreset,
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

  it("normalizes negotiated video codec and encoder fields", () => {
    expect(
      normalizePreset({
        videoCodec: "h265",
        videoEncoder: "c2.vendor.hevc.encoder",
      }),
    ).toMatchObject({
      videoCodec: "h265",
      videoEncoder: "c2.vendor.hevc.encoder",
    });
    expect(
      normalizePreset({
        videoCodec: "bad" as never,
        videoEncoder: "bad encoder with spaces",
      }),
    ).toMatchObject({ videoCodec: "h264", videoEncoder: "" });
  });

  it("drops legacy renderer-authored recording paths", () => {
    const migrated = normalizePreset({
      recording: true,
      recordPath: "C:/legacy/arbitrary.mp4",
    } as Partial<MirrorPreset> & { recordPath: string });

    expect(migrated.recording).toBe(true);
    expect(migrated).not.toHaveProperty("recordPath");
  });

  it("scopes saved presets to each device serial", () => {
    expect(presetStorageKey("DEVICE123")).toBe(
      "droidsmith.mirror.preset.DEVICE123",
    );
  });
});
