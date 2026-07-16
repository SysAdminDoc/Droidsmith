export type KeyboardMode = "default" | "sdk" | "uhid" | "aoa" | "disabled";
export type VideoCodec = "h264" | "h265" | "av1" | "vp8" | "vp9";

export type MirrorPreset = {
  maxSize: string;
  bitRate: string;
  noAudio: boolean;
  recording: boolean;
  keyboardMode: KeyboardMode;
  videoCodec: VideoCodec;
  videoEncoder: string;
  turnScreenOff: boolean;
  stayAwake: boolean;
  showTouches: boolean;
};

export const DEFAULT_MIRROR_PRESET: MirrorPreset = {
  maxSize: "1024",
  bitRate: "8M",
  noAudio: false,
  recording: false,
  keyboardMode: "default",
  videoCodec: "h264",
  videoEncoder: "",
  turnScreenOff: false,
  stayAwake: false,
  showTouches: false,
};

export const LEGACY_MIRROR_PRESET_PREFIX = "droidsmith.mirror.preset.";

export function normalizePreset(value: Partial<MirrorPreset>): MirrorPreset {
  return {
    maxSize:
      typeof value.maxSize === "string"
        ? value.maxSize
        : DEFAULT_MIRROR_PRESET.maxSize,
    bitRate:
      typeof value.bitRate === "string"
        ? value.bitRate
        : DEFAULT_MIRROR_PRESET.bitRate,
    noAudio:
      typeof value.noAudio === "boolean"
        ? value.noAudio
        : DEFAULT_MIRROR_PRESET.noAudio,
    recording:
      typeof value.recording === "boolean"
        ? value.recording
        : DEFAULT_MIRROR_PRESET.recording,
    keyboardMode: isKeyboardMode(value.keyboardMode)
      ? value.keyboardMode
      : DEFAULT_MIRROR_PRESET.keyboardMode,
    videoCodec: isVideoCodec(value.videoCodec)
      ? value.videoCodec
      : DEFAULT_MIRROR_PRESET.videoCodec,
    videoEncoder:
      typeof value.videoEncoder === "string" &&
      /^[A-Za-z0-9_.-]{0,255}$/u.test(value.videoEncoder)
        ? value.videoEncoder
        : DEFAULT_MIRROR_PRESET.videoEncoder,
    turnScreenOff:
      typeof value.turnScreenOff === "boolean"
        ? value.turnScreenOff
        : DEFAULT_MIRROR_PRESET.turnScreenOff,
    stayAwake:
      typeof value.stayAwake === "boolean"
        ? value.stayAwake
        : DEFAULT_MIRROR_PRESET.stayAwake,
    showTouches:
      typeof value.showTouches === "boolean"
        ? value.showTouches
        : DEFAULT_MIRROR_PRESET.showTouches,
  };
}

function isVideoCodec(value: unknown): value is VideoCodec {
  return (
    value === "h264" ||
    value === "h265" ||
    value === "av1" ||
    value === "vp8" ||
    value === "vp9"
  );
}

export function presetStorageKey(serial: string): string {
  return `${LEGACY_MIRROR_PRESET_PREFIX}${serial}`;
}

function isKeyboardMode(value: unknown): value is KeyboardMode {
  return (
    value === "default" ||
    value === "sdk" ||
    value === "uhid" ||
    value === "aoa" ||
    value === "disabled"
  );
}
