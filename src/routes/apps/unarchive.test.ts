import { describe, expect, it } from "vitest";

import { archiveIsRisky, classifyUnarchive } from "./unarchive";

describe("classifyUnarchive", () => {
  const installed = new Set(["com.android.vending", "org.fdroid.fdroid"]);

  it("treats a Play Store installer as reversible", () => {
    const outlook = classifyUnarchive("com.android.vending", installed);
    expect(outlook).toBe("capable");
    expect(archiveIsRisky(outlook)).toBe(false);
  });

  it("treats a null installer as sideloaded and risky", () => {
    const outlook = classifyUnarchive(null, installed);
    expect(outlook).toBe("sideloaded");
    expect(archiveIsRisky(outlook)).toBe(true);
  });

  it("treats adb/package-installer sources as sideloaded", () => {
    expect(classifyUnarchive("com.android.shell", installed)).toBe(
      "sideloaded",
    );
    expect(
      classifyUnarchive("com.google.android.packageinstaller", installed),
    ).toBe("sideloaded");
    expect(classifyUnarchive("com.android.packageinstaller", installed)).toBe(
      "sideloaded",
    );
  });

  it("flags a recorded installer that is no longer installed", () => {
    const outlook = classifyUnarchive("com.example.oldstore", installed);
    expect(outlook).toBe("installer_missing");
    expect(archiveIsRisky(outlook)).toBe(true);
  });

  it("marks a present third-party installer as unverified (still risky)", () => {
    const outlook = classifyUnarchive("org.fdroid.fdroid", installed);
    expect(outlook).toBe("unverified");
    expect(archiveIsRisky(outlook)).toBe(true);
  });

  it("handles an undefined installer like a missing one", () => {
    expect(classifyUnarchive(undefined, installed)).toBe("sideloaded");
  });
});
