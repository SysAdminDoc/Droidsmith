import { describe, expect, it } from "vitest";

import {
  canRunLegacyExport,
  packageExportDefaultFileName,
  packageExportDisplayState,
} from "./appsBackup";

describe("Apps route destructive export guards", () => {
  it("keeps blocked legacy exports unavailable", () => {
    expect(
      canRunLegacyExport({ legacy_capability: "legacy_data_blocked" } as never),
    ).toBe(false);
    expect(
      canRunLegacyExport({
        legacy_capability: "legacy_data_available",
      } as never),
    ).toBe(true);
  });

  it("keeps export labels and filenames deterministic", () => {
    expect(
      packageExportDisplayState({ manifest: { mode: "apk_export" } } as never),
    ).toBe("apk_exported");
    expect(packageExportDefaultFileName("bad/pkg", "apk_export")).toBe(
      "bad_pkg.apks.zip",
    );
  });
});
