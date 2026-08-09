import { describe, expect, it } from "vitest";

import { getRendererRecoveryFallbackCopy } from "./lib/rendererRecovery";

describe("renderer recovery fallback", () => {
  it("keeps static recovery copy when the i18n tree itself fails", () => {
    const copy = getRendererRecoveryFallbackCopy(() => "");

    expect(copy).toEqual({
      title: "Droidsmith could not render its recovery controls.",
      body: "Close and reopen Droidsmith to continue.",
    });
  });
});
