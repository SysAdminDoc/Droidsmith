export type KeyboardMode = "default" | "sdk" | "uhid" | "aoa" | "disabled";

export type MirrorPreset = {
  maxSize: string;
  bitRate: string;
  noAudio: boolean;
  recording: boolean;
  recordPath: string;
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
  recordPath: "",
  keyboardMode: "default",
  turnScreenOff: false,
  stayAwake: false,
  showTouches: false,
};

export function normalizePreset(value: Partial<MirrorPreset>): MirrorPreset {
  return {
    ...DEFAULT_MIRROR_PRESET,
    ...value,
    keyboardMode: isKeyboardMode(value.keyboardMode)
      ? value.keyboardMode
      : DEFAULT_MIRROR_PRESET.keyboardMode,
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
