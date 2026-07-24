// R-109: classify whether an Android 15 `archive` can be reversed via the OS
// `request-unarchive` flow. Unarchive dispatches an `UNARCHIVE_PACKAGE` intent
// to the package's installer-of-record; if that installer is absent or does not
// implement the flow, an archived package cannot be restored by the OS. The
// installer id is already captured per package from `pm list packages -i`.

/** Outlook for reversing an archive of one package. */
export type UnarchiveOutlook =
  // Installer-of-record is the Play Store, which drives archive/unarchive.
  | "capable"
  // No installer-of-record, or a known non-store installer (adb / package
  // installer). Not restorable through the OS unarchive flow.
  | "sideloaded"
  // An installer-of-record is recorded but that installer is no longer
  // installed, so the unarchive intent has no handler.
  | "installer_missing"
  // A third-party installer is present but its unarchive support is unknown.
  | "unverified";

/** The one installer Android's archive/unarchive flow is designed around. */
const PLAY_STORE = "com.android.vending";

/** Installers that never implement the unarchive intent (sideload paths). */
const NON_STORE_INSTALLERS: ReadonlySet<string> = new Set([
  "com.android.shell",
  "com.google.android.packageinstaller",
  "com.android.packageinstaller",
]);

/**
 * Classify the unarchive outlook for a package given its installer-of-record
 * and the set of package ids currently installed on the device (used to detect
 * an installer that has since been removed).
 */
export function classifyUnarchive(
  installer: string | null | undefined,
  installedPackageIds: ReadonlySet<string>,
): UnarchiveOutlook {
  if (installer === PLAY_STORE) return "capable";
  if (!installer || NON_STORE_INSTALLERS.has(installer)) return "sideloaded";
  return installedPackageIds.has(installer)
    ? "unverified"
    : "installer_missing";
}

/**
 * True when archiving cannot be reliably reversed by the OS. Only a Play
 * Store installer-of-record is treated as cleanly reversible.
 */
export function archiveIsRisky(outlook: UnarchiveOutlook): boolean {
  return outlook !== "capable";
}
