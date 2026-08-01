import type { UninstallRecoveryEvidence } from "../../lib/bindings";

/**
 * Present a pre-mutation reinstall-feasibility verdict.
 *
 * The whole point of R-122 is that `unknown` never reads as reassurance. A
 * verdict the device could not supply is shown with the same weight as a
 * refusal, because acting on a wrong "you can undo this" costs the user a
 * package they cannot get back.
 */
export type RecoveryPresentation = {
  tone: "success" | "danger" | "warning";
  titleKey: string;
  detailKey: string;
  /** True when the user is about to do something they cannot reverse. */
  irreversible: boolean;
};

const REASON_KEYS: Record<string, string> = {
  platform_apk_retained: "apps.recovery.reason.platformApkRetained",
  only_copy_is_user_installed: "apps.recovery.reason.onlyCopyUserInstalled",
  package_not_installed_for_user: "apps.recovery.reason.notInstalledForUser",
  system_flag_conflicts_with_apk_path:
    "apps.recovery.reason.conflictingEvidence",
  system_flag_probe_failed: "apps.recovery.reason.probeFailed",
  probe_failed: "apps.recovery.reason.probeFailed",
  invalid_package_name: "apps.recovery.reason.probeFailed",
};

const UNKNOWN_DETAIL = "apps.recovery.reason.probeFailed";

export function presentRecovery(
  evidence: UninstallRecoveryEvidence | null | undefined,
): RecoveryPresentation | null {
  if (!evidence) return null;
  const detailKey = REASON_KEYS[evidence.reason_code] ?? UNKNOWN_DETAIL;
  switch (evidence.verdict) {
    case "recoverable":
      return {
        tone: "success",
        titleKey: "apps.recovery.recoverable",
        detailKey,
        irreversible: false,
      };
    case "not_recoverable":
      return {
        tone: "danger",
        titleKey: "apps.recovery.notRecoverable",
        detailKey,
        irreversible: true,
      };
    default:
      // Deliberately grouped with "cannot be undone" for the purposes of the
      // warning the user reads, while staying honestly labelled as unproven.
      return {
        tone: "warning",
        titleKey: "apps.recovery.unknown",
        detailKey,
        irreversible: true,
      };
  }
}
