import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

import { describe, expect, it } from "vitest";

import {
  compositeColor,
  contrastRatio,
  parseCssColor,
  resolveRenderedColors,
} from "./contrast";

describe("accessible color tokens", () => {
  it("keeps muted normal text at WCAG AA contrast on elevated surfaces", () => {
    const anvil = {
      "400": "#91a2b2",
      "500": "#8497a8",
      "600": "#7d91a2",
      "800": "#1a2631",
    };

    for (const token of ["400", "500", "600"] as const) {
      expect(contrastRatio(anvil[token], anvil["800"])).toBeGreaterThanOrEqual(
        4.5,
      );
    }
  });

  it("composites translucent rendered surfaces before measuring contrast", () => {
    const rendered = resolveRenderedColors("rgb(255 255 255 / 0.72)", [
      "rgb(255 255 255 / 0.08)",
      "rgb(8 15 22)",
    ]);
    expect(rendered.background.alpha).toBe(1);
    expect(contrastRatio(rendered.foreground, rendered.background)).toBeCloseTo(
      8.89,
      2,
    );
  });

  it("parses browser rgb forms and composites alpha deterministically", () => {
    expect(parseCssColor("rgba(255, 255, 255, 0.5)")).toEqual({
      red: 255,
      green: 255,
      blue: 255,
      alpha: 0.5,
    });
    expect(
      compositeColor(
        parseCssColor("rgb(255 255 255 / 0.5)"),
        parseCssColor("#000000"),
      ),
    ).toEqual({ red: 127.5, green: 127.5, blue: 127.5, alpha: 1 });
    expect(parseCssColor("color(srgb 0.25 0.5 0.75 / 0.8)")).toEqual({
      red: 63.75,
      green: 127.5,
      blue: 191.25,
      alpha: 0.8,
    });
  });

  it("keeps a complete light semantic palette for the rendered gate", () => {
    const css = readFileSync(
      fileURLToPath(new URL("../index.css", import.meta.url)),
      "utf8",
    );
    const lightStart = css.indexOf(':root[data-theme="light"]');
    const lightEnd = css.indexOf("html,", lightStart);
    const lightBlock = css.slice(lightStart, lightEnd);
    expect(lightStart).toBeGreaterThanOrEqual(0);
    for (const token of [
      "--ds-anvil-50",
      "--ds-anvil-600",
      "--ds-anvil-950",
      "--ds-circuit-300",
      "--ds-signal-green",
      "--ds-signal-amber",
      "--ds-signal-red",
      "--ds-surface-card",
      "--ds-surface-dialog",
      "--ds-surface-terminal",
    ]) {
      expect(lightBlock).toContain(token);
    }
    expect(css).toContain(".bg-surface-terminal");
  });
});
