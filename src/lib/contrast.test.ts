import { describe, expect, it } from "vitest";

import tailwindConfig from "../../tailwind.config";
import {
  compositeColor,
  contrastRatio,
  parseCssColor,
  resolveRenderedColors,
} from "./contrast";

describe("accessible color tokens", () => {
  it("keeps muted normal text at WCAG AA contrast on elevated surfaces", () => {
    const anvil = tailwindConfig.theme.extend.colors.anvil;

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
  });
});
