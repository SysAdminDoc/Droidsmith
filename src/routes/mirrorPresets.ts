export type KeyboardMode = "default" | "sdk" | "uhid" | "aoa" | "disabled";

export type MirrorPreset = {
  maxSize: string;
  bitRate: string;
  noAudio: boolean;
  recording: boolean;
  keyboardMode: KeyboardMode;
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
  turnScreenOff: false,
  stayAwake: false,
  showTouches: false,
};

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

export function presetStorageKey(serial: string): string {
  return `droidsmith.mirror.preset.${serial}`;
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
