// R-114: compare two already-parsed APK analyses and report the deltas that
// matter when reviewing an app update — permissions, components, SDK levels,
// signing schemes, and signer certificates. Pure and fully offline: it never
// touches the device or the filesystem, so it is fixture-testable in vitest and
// its output stays local like the rest of the analyzer.
import type { ApkAnalysis } from "../lib/tauri";

export type ApkVersionRef = {
  fileName: string;
  package: string | null;
  versionName: string | null;
  versionCode: number | null;
};

/** A changed integer field (component counts). */
export type CountDelta = { key: string; base: number; other: number };

/** A changed SDK level, where either side may be undeclared (null). */
export type SdkDelta = {
  key: string;
  base: number | null;
  other: number | null;
};

/** A changed signing scheme flag. */
export type SigningSchemeDelta = {
  scheme: string;
  base: boolean;
  other: boolean;
};

export type ApkDiff = {
  base: ApkVersionRef;
  other: ApkVersionRef;
  /** True when both APKs declare the same package id. */
  samePackage: boolean;
  permissionsAdded: string[];
  permissionsRemoved: string[];
  componentDeltas: CountDelta[];
  sdkDeltas: SdkDelta[];
  signingChanges: SigningSchemeDelta[];
  /** Signer certificate SHA-256 digests present only in the second APK. */
  signerCertsAdded: string[];
  /** Signer certificate SHA-256 digests present only in the first APK. */
  signerCertsRemoved: string[];
  /** `other.file_size - base.file_size` in bytes (may be negative). */
  fileSizeDelta: number;
  /** True when nothing tracked here changed between the two APKs. */
  identical: boolean;
};

const COMPONENT_KEYS = [
  "activities",
  "services",
  "receivers",
  "providers",
] as const;
const SIGNING_SCHEMES = ["v1", "v2", "v3", "v31"] as const;

function versionRef(analysis: ApkAnalysis): ApkVersionRef {
  return {
    fileName: analysis.file_name,
    package: analysis.package,
    versionName: analysis.version_name,
    versionCode: analysis.version_code,
  };
}

/** Diff two parsed APK analyses (base → other). */
export function diffApkAnalyses(
  base: ApkAnalysis,
  other: ApkAnalysis,
): ApkDiff {
  const basePermissions = new Set(base.permissions);
  const otherPermissions = new Set(other.permissions);
  const permissionsAdded = other.permissions.filter(
    (permission) => !basePermissions.has(permission),
  );
  const permissionsRemoved = base.permissions.filter(
    (permission) => !otherPermissions.has(permission),
  );

  const componentDeltas: CountDelta[] = COMPONENT_KEYS.map((key) => ({
    key,
    base: base.components[key],
    other: other.components[key],
  })).filter((delta) => delta.base !== delta.other);

  const sdkPairs: SdkDelta[] = [
    { key: "min", base: base.min_sdk, other: other.min_sdk },
    { key: "target", base: base.target_sdk, other: other.target_sdk },
    { key: "compile", base: base.compile_sdk, other: other.compile_sdk },
  ];
  const sdkDeltas = sdkPairs.filter((delta) => delta.base !== delta.other);

  const signingChanges: SigningSchemeDelta[] = SIGNING_SCHEMES.map(
    (scheme) => ({
      scheme,
      base: base.signing[scheme],
      other: other.signing[scheme],
    }),
  ).filter((delta) => delta.base !== delta.other);

  const baseCerts = new Set(
    base.signature_verification.signers.map((signer) => signer.sha256),
  );
  const otherCerts = new Set(
    other.signature_verification.signers.map((signer) => signer.sha256),
  );
  const signerCertsAdded = [...otherCerts].filter(
    (cert) => !baseCerts.has(cert),
  );
  const signerCertsRemoved = [...baseCerts].filter(
    (cert) => !otherCerts.has(cert),
  );

  const fileSizeDelta = other.file_size - base.file_size;

  const identical =
    permissionsAdded.length === 0 &&
    permissionsRemoved.length === 0 &&
    componentDeltas.length === 0 &&
    sdkDeltas.length === 0 &&
    signingChanges.length === 0 &&
    signerCertsAdded.length === 0 &&
    signerCertsRemoved.length === 0 &&
    (base.package ?? null) === (other.package ?? null) &&
    (base.version_code ?? null) === (other.version_code ?? null) &&
    (base.version_name ?? null) === (other.version_name ?? null) &&
    fileSizeDelta === 0;

  return {
    base: versionRef(base),
    other: versionRef(other),
    samePackage: (base.package ?? null) === (other.package ?? null),
    permissionsAdded,
    permissionsRemoved,
    componentDeltas,
    sdkDeltas,
    signingChanges,
    signerCertsAdded,
    signerCertsRemoved,
    fileSizeDelta,
    identical,
  };
}
