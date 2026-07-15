import { describe, expect, it } from "vitest";

import type { PackageBackupPreflight, PackageExportResult } from "../lib/tauri";
import {
  canRunLegacyExport,
  formatBackupSize,
  packageExportDefaultFileName,
  packageExportDisplayState,
} from "./appsBackup";

function result(
  mode: "apk_export" | "legacy_data",
  legacyContent: PackageExportResult["manifest"]["legacy_content"] = null,
): PackageExportResult {
  return {
    artifact: { local_path: "C:/exports/app.zip", size_bytes: 42, sha256: "a" },
    manifest: {
      format: "droidsmith_package_export",
      schema_version: 1,
      created_at: "2026-07-15T00:00:00Z",
      mode,
      package: "com.example.app",
      android_user: 10,
      device: {
        device_identity_sha256: "b".repeat(64),
        build_identity_sha256: "c".repeat(64),
      },
      eligibility: {
        device_sdk: 35,
        target_sdk: 35,
        debuggable: false,
        allow_backup: true,
        reason: "fixture",
      },
      legacy_content: legacyContent,
      artifacts: [],
    },
  };
}

describe("apps package export helpers", () => {
  it("distinguishes APK export from inspected legacy payload states", () => {
    expect(packageExportDisplayState(result("apk_export"))).toBe(
      "apk_exported",
    );
    expect(
      packageExportDisplayState(
        result("legacy_data", "app_data_entries_detected"),
      ),
    ).toBe("legacy_entries_detected");
    expect(
      packageExportDisplayState(result("legacy_data", "no_app_data_entries")),
    ).toBe("legacy_no_data");
  });

  it("builds distinct safe ZIP names", () => {
    expect(packageExportDefaultFileName("com.example.app", "apk_export")).toBe(
      "com.example.app.apks.zip",
    );
    expect(
      packageExportDefaultFileName("bad/package:name", "legacy_data"),
    ).toBe("bad_package_name.legacy-data.zip");
  });

  it("blocks only evidence-backed legacy exclusions", () => {
    const preflight = {
      legacy_capability: "legacy_data_blocked",
    } as PackageBackupPreflight;
    expect(canRunLegacyExport(preflight)).toBe(false);
    expect(
      canRunLegacyExport({
        ...preflight,
        legacy_capability: "legacy_data_unknown",
      }),
    ).toBe(true);
  });

  it("formats artifact sizes without hiding zero-byte results", () => {
    expect(formatBackupSize(null)).toBeNull();
    expect(formatBackupSize(0)).toBe("0 B");
    expect(formatBackupSize(512)).toBe("512 B");
    expect(formatBackupSize(1536)).toBe("1.5 KiB");
    expect(formatBackupSize(2 * 1024 * 1024)).toBe("2.0 MiB");
  });
});
