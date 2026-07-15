import { describe, expect, it } from "vitest";

import tailwindConfig from "../../tailwind.config";

describe("accessible color tokens", () => {
  it("keeps muted normal text at WCAG AA contrast on elevated surfaces", () => {
    const anvil = tailwindConfig.theme.extend.colors.anvil;

    for (const token of ["400", "500", "600"] as const) {
      expect(contrastRatio(anvil[token], anvil["800"])).toBeGreaterThanOrEqual(
        4.5,
      );
    }
  });
});

function contrastRatio(foreground: string, background: string): number {
  const foregroundLuminance = relativeLuminance(foreground);
  const backgroundLuminance = relativeLuminance(background);
  const lightest = Math.max(foregroundLuminance, backgroundLuminance);
  const darkest = Math.min(foregroundLuminance, backgroundLuminance);
  return (lightest + 0.05) / (darkest + 0.05);
}

function relativeLuminance(hex: string): number {
  const channels = hex
    .slice(1)
    .match(/.{2}/g)!
    .map((channel) => Number.parseInt(channel, 16) / 255)
    .map((channel) =>
      channel <= 0.03928 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4,
    );
  return channels[0]! * 0.2126 + channels[1]! * 0.7152 + channels[2]! * 0.0722;
}
