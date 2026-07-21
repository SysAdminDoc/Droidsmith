export type KeyboardMode = "default" | "sdk" | "uhid" | "aoa" | "disabled";
export type VideoCodec = "h264" | "h265" | "av1" | "vp8" | "vp9";
export type AudioCodec = "default" | "opus" | "aac" | "flac" | "raw";
export type DisplayOrientation =
  | ""
  | "0"
  | "90"
  | "180"
  | "270"
  | "flip0"
  | "flip90"
  | "flip180"
  | "flip270";

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
  flexDisplay: boolean;
  keepActive: boolean;
  maxFps: string;
  fullscreen: boolean;
  alwaysOnTop: boolean;
  noControl: boolean;
  crop: string;
  displayOrientation: string;
  screenOffTimeout: string;
  audioCodec: AudioCodec;
  newDisplay: string;
  audioSource: string;
  videoSource: string;
  cameraFacing: string;
  cameraSize: string;
};

export type AudioSource =
  | "output"
  | "mic"
  | "mic-unprocessed"
  | "mic-voice-communication"
  | "mic-voice-recognition"
  | "voice-call"
  | "playback";

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
  flexDisplay: false,
  keepActive: false,
  maxFps: "",
  fullscreen: false,
  alwaysOnTop: false,
  noControl: false,
  crop: "",
  displayOrientation: "",
  screenOffTimeout: "",
  audioCodec: "default",
  newDisplay: "",
  audioSource: "output",
  videoSource: "display",
  cameraFacing: "back",
  cameraSize: "",
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
    flexDisplay:
      typeof value.flexDisplay === "boolean"
        ? value.flexDisplay
        : DEFAULT_MIRROR_PRESET.flexDisplay,
    keepActive:
      typeof value.keepActive === "boolean"
        ? value.keepActive
        : DEFAULT_MIRROR_PRESET.keepActive,
    maxFps:
      typeof value.maxFps === "string" && /^\d{0,3}$/u.test(value.maxFps)
        ? value.maxFps
        : DEFAULT_MIRROR_PRESET.maxFps,
    fullscreen:
      typeof value.fullscreen === "boolean"
        ? value.fullscreen
        : DEFAULT_MIRROR_PRESET.fullscreen,
    alwaysOnTop:
      typeof value.alwaysOnTop === "boolean"
        ? value.alwaysOnTop
        : DEFAULT_MIRROR_PRESET.alwaysOnTop,
    noControl:
      typeof value.noControl === "boolean"
        ? value.noControl
        : DEFAULT_MIRROR_PRESET.noControl,
    crop:
      typeof value.crop === "string" &&
      /^(\d{1,6}:\d{1,6}:\d{1,6}:\d{1,6})?$/u.test(value.crop)
        ? value.crop
        : DEFAULT_MIRROR_PRESET.crop,
    displayOrientation: isDisplayOrientation(value.displayOrientation)
      ? value.displayOrientation
      : DEFAULT_MIRROR_PRESET.displayOrientation,
    screenOffTimeout:
      typeof value.screenOffTimeout === "string" &&
      /^\d{0,6}$/u.test(value.screenOffTimeout)
        ? value.screenOffTimeout
        : DEFAULT_MIRROR_PRESET.screenOffTimeout,
    audioCodec: isAudioCodec(value.audioCodec)
      ? value.audioCodec
      : DEFAULT_MIRROR_PRESET.audioCodec,
    newDisplay:
      typeof value.newDisplay === "string" &&
      /^((\d{1,5}x\d{1,5})(\/\d{1,5})?|\/\d{1,5})?$/u.test(value.newDisplay)
        ? value.newDisplay
        : DEFAULT_MIRROR_PRESET.newDisplay,
    audioSource: isAudioSource(value.audioSource)
      ? value.audioSource
      : DEFAULT_MIRROR_PRESET.audioSource,
    videoSource:
      value.videoSource === "camera"
        ? "camera"
        : DEFAULT_MIRROR_PRESET.videoSource,
    cameraFacing:
      value.cameraFacing === "front" ||
      value.cameraFacing === "back" ||
      value.cameraFacing === "external"
        ? value.cameraFacing
        : DEFAULT_MIRROR_PRESET.cameraFacing,
    cameraSize:
      typeof value.cameraSize === "string" &&
      /^(\d{1,5}x\d{1,5})?$/u.test(value.cameraSize)
        ? value.cameraSize
        : DEFAULT_MIRROR_PRESET.cameraSize,
  };
}

function isAudioSource(value: unknown): value is AudioSource {
  return (
    value === "output" ||
    value === "mic" ||
    value === "mic-unprocessed" ||
    value === "mic-voice-communication" ||
    value === "mic-voice-recognition" ||
    value === "voice-call" ||
    value === "playback"
  );
}

function isAudioCodec(value: unknown): value is AudioCodec {
  return (
    value === "default" ||
    value === "opus" ||
    value === "aac" ||
    value === "flac" ||
    value === "raw"
  );
}

function isDisplayOrientation(value: unknown): value is DisplayOrientation {
  return (
    value === "" ||
    value === "0" ||
    value === "90" ||
    value === "180" ||
    value === "270" ||
    value === "flip0" ||
    value === "flip90" ||
    value === "flip180" ||
    value === "flip270"
  );
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
