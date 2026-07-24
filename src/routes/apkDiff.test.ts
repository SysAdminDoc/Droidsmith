import { describe, expect, it } from "vitest";

import type { ApkAnalysis } from "../lib/tauri";
import { diffApkAnalyses } from "./apkDiff";

function signer(
  sha256: string,
): ApkAnalysis["signature_verification"]["signers"][number] {
  return {
    label: "Signer #1",
    sha256,
    subject: "CN=Example",
    issuer: "CN=Example",
    valid_from_unix: 0,
    valid_until_unix: 0,
  };
}

function analysis(overrides: Partial<ApkAnalysis> = {}): ApkAnalysis {
  return {
    file_name: "app.apk",
    file_size: 1000,
    sha256: "0".repeat(64),
    package: "com.example.app",
    version_code: 1,
    version_name: "1.0",
    min_sdk: 24,
    target_sdk: 33,
    compile_sdk: 34,
    permissions: ["android.permission.INTERNET"],
    components: { activities: 2, services: 1, receivers: 0, providers: 0 },
    dex: {
      files: 1,
      defined_classes: 10,
      method_refs: 100,
      exceeds_64k: false,
    },
    signing: { v1: false, v2: true, v3: true, v31: false },
    signature_verification: {
      status: "not_verified",
      unavailable_reason: null,
      tool: null,
      verified_schemes: [],
      signer_count: 0,
      signers: [],
      source_stamp_verified: false,
      source_stamp: null,
      proof_of_rotation: null,
      warnings: [],
      errors: [],
    },
    total_entries: 5,
    largest_entries: [],
    ...overrides,
  };
}

describe("diffApkAnalyses", () => {
  it("reports no changes for identical analyses", () => {
    const diff = diffApkAnalyses(analysis(), analysis());
    expect(diff.identical).toBe(true);
    expect(diff.permissionsAdded).toEqual([]);
    expect(diff.permissionsRemoved).toEqual([]);
    expect(diff.fileSizeDelta).toBe(0);
  });

  it("reports permission additions and removals", () => {
    const base = analysis({
      permissions: ["android.permission.INTERNET", "android.permission.CAMERA"],
    });
    const other = analysis({
      permissions: [
        "android.permission.INTERNET",
        "android.permission.RECORD_AUDIO",
      ],
    });
    const diff = diffApkAnalyses(base, other);
    expect(diff.permissionsAdded).toEqual(["android.permission.RECORD_AUDIO"]);
    expect(diff.permissionsRemoved).toEqual(["android.permission.CAMERA"]);
    expect(diff.identical).toBe(false);
  });

  it("reports component, sdk, size, and version deltas", () => {
    const base = analysis();
    const other = analysis({
      version_code: 2,
      version_name: "1.1",
      file_size: 1500,
      target_sdk: 34,
      compile_sdk: 34,
      components: { activities: 3, services: 1, receivers: 0, providers: 0 },
    });
    const diff = diffApkAnalyses(base, other);
    expect(diff.componentDeltas).toEqual([
      { key: "activities", base: 2, other: 3 },
    ]);
    expect(diff.sdkDeltas).toEqual([{ key: "target", base: 33, other: 34 }]);
    expect(diff.fileSizeDelta).toBe(500);
    expect(diff.other.versionCode).toBe(2);
    expect(diff.identical).toBe(false);
  });

  it("detects signing-scheme and signer-certificate changes", () => {
    const base = analysis({
      signing: { v1: false, v2: true, v3: false, v31: false },
      signature_verification: {
        ...analysis().signature_verification,
        signers: [signer("aaaa")],
      },
    });
    const other = analysis({
      signing: { v1: false, v2: true, v3: true, v31: false },
      signature_verification: {
        ...analysis().signature_verification,
        signers: [signer("bbbb")],
      },
    });
    const diff = diffApkAnalyses(base, other);
    expect(diff.signingChanges).toEqual([
      { scheme: "v3", base: false, other: true },
    ]);
    expect(diff.signerCertsAdded).toEqual(["bbbb"]);
    expect(diff.signerCertsRemoved).toEqual(["aaaa"]);
  });

  it("flags a package-id mismatch", () => {
    const diff = diffApkAnalyses(
      analysis(),
      analysis({ package: "com.other.app" }),
    );
    expect(diff.samePackage).toBe(false);
  });
});
