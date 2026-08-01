import { describe, expect, it } from "vitest";

import { presentRecovery } from "./uninstallRecovery";

describe("presentRecovery", () => {
  it("marks a proven system package as reversible", () => {
    const shown = presentRecovery({
      verdict: "recoverable",
      reason_code: "platform_apk_retained",
      apk_path: "/system/app/A/A.apk",
    });
    expect(shown).toEqual({
      tone: "success",
      titleKey: "apps.recovery.recoverable",
      detailKey: "apps.recovery.reason.platformApkRetained",
      irreversible: false,
    });
  });

  it("never presents an unproven verdict as reversible", () => {
    for (const reason_code of [
      "probe_failed",
      "system_flag_probe_failed",
      "system_flag_conflicts_with_apk_path",
      "package_not_installed_for_user",
      "a code this build has never heard of",
    ]) {
      const shown = presentRecovery({
        verdict: "unknown",
        reason_code,
        apk_path: null,
      });
      expect(shown?.irreversible).toBe(true);
      expect(shown?.titleKey).toBe("apps.recovery.unknown");
      // An unrecognised code still resolves to a real string rather than
      // rendering the raw code at the user.
      expect(shown?.detailKey.startsWith("apps.recovery.reason.")).toBe(true);
    }
  });

  it("flags a user-installed package as unrecoverable", () => {
    const shown = presentRecovery({
      verdict: "not_recoverable",
      reason_code: "only_copy_is_user_installed",
      apk_path: "/data/app/~~a==/com.example.app-b==/base.apk",
    });
    expect(shown?.tone).toBe("danger");
    expect(shown?.irreversible).toBe(true);
  });

  it("renders nothing when no assessment was made", () => {
    expect(presentRecovery(null)).toBeNull();
    expect(presentRecovery(undefined)).toBeNull();
  });
});
