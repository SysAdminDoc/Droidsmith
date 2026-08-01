export type RgbaColor = Readonly<{
  red: number;
  green: number;
  blue: number;
  alpha: number;
}>;

const HEX_COLOR = /^#([\da-f]{6}|[\da-f]{8})$/iu;
const RGB_COLOR = /^rgba?\((.*)\)$/iu;

export function parseCssColor(value: string): RgbaColor {
  const normalized = value.trim().toLowerCase();
  if (normalized === "transparent") {
    return { red: 0, green: 0, blue: 0, alpha: 0 };
  }

  const hex = normalized.match(HEX_COLOR)?.[1];
  if (hex) {
    return {
      red: Number.parseInt(hex.slice(0, 2), 16),
      green: Number.parseInt(hex.slice(2, 4), 16),
      blue: Number.parseInt(hex.slice(4, 6), 16),
      alpha: hex.length === 8 ? Number.parseInt(hex.slice(6, 8), 16) / 255 : 1,
    };
  }

  const rgb = normalized.match(RGB_COLOR)?.[1];
  if (!rgb) throw new Error(`Unsupported CSS color: ${value}`);
  const [channelsPart, slashAlpha] = rgb.split("/").map((part) => part.trim());
  const commaParts = (channelsPart ?? "")
    .split(",")
    .map((part) => part.trim())
    .filter(Boolean);
  const channelParts =
    commaParts.length > 1
      ? commaParts.slice(0, 3)
      : (channelsPart ?? "").split(/\s+/u).filter(Boolean);
  const channels = channelParts.map(Number);
  if (
    channels.length !== 3 ||
    channels.some((channel) => !Number.isFinite(channel))
  ) {
    throw new Error(`Unsupported CSS color: ${value}`);
  }
  const alphaValue = slashAlpha ?? commaParts[3];
  const alpha = alphaValue === undefined ? 1 : Number(alphaValue);
  if (!Number.isFinite(alpha))
    throw new Error(`Unsupported CSS color: ${value}`);

  return {
    red: channels[0]!,
    green: channels[1]!,
    blue: channels[2]!,
    alpha,
  };
}

export function compositeColor(
  foreground: RgbaColor,
  background: RgbaColor,
): RgbaColor {
  const alpha = foreground.alpha + background.alpha * (1 - foreground.alpha);
  if (alpha === 0) return { red: 0, green: 0, blue: 0, alpha: 0 };
  const mix = (foregroundChannel: number, backgroundChannel: number) =>
    (foregroundChannel * foreground.alpha +
      backgroundChannel * background.alpha * (1 - foreground.alpha)) /
    alpha;
  return {
    red: mix(foreground.red, background.red),
    green: mix(foreground.green, background.green),
    blue: mix(foreground.blue, background.blue),
    alpha,
  };
}

export function resolveRenderedColors(
  foreground: string,
  backgroundLayersNearestFirst: readonly string[],
): { foreground: RgbaColor; background: RgbaColor } {
  let background: RgbaColor = { red: 255, green: 255, blue: 255, alpha: 1 };
  for (const layer of [...backgroundLayersNearestFirst].reverse()) {
    background = compositeColor(parseCssColor(layer), background);
  }
  return {
    foreground: compositeColor(parseCssColor(foreground), background),
    background,
  };
}

export function contrastRatio(
  foreground: RgbaColor | string,
  background: RgbaColor | string,
): number {
  const foregroundColor =
    typeof foreground === "string" ? parseCssColor(foreground) : foreground;
  const backgroundColor =
    typeof background === "string" ? parseCssColor(background) : background;
  const foregroundLuminance = relativeLuminance(foregroundColor);
  const backgroundLuminance = relativeLuminance(backgroundColor);
  const lightest = Math.max(foregroundLuminance, backgroundLuminance);
  const darkest = Math.min(foregroundLuminance, backgroundLuminance);
  return (lightest + 0.05) / (darkest + 0.05);
}

function relativeLuminance(color: RgbaColor): number {
  const channels = [color.red, color.green, color.blue]
    .map((channel) => channel / 255)
    .map((channel) =>
      channel <= 0.04045 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4,
    );
  return channels[0]! * 0.2126 + channels[1]! * 0.7152 + channels[2]! * 0.0722;
}
