import { describe, expect, it } from "vitest";

import {
  backupDefaultFileName,
  backupDisplayState,
  formatBackupSize,
} from "./appsBackup";

describe("apps backup helpers", () => {
  it("classifies zero-byte or missing backup artifacts as empty", () => {
    expect(backupDisplayState({ empty: true, size_bytes: null })).toBe("empty");
    expect(backupDisplayState({ empty: false, size_bytes: 0 })).toBe("empty");
    expect(backupDisplayState({ empty: false, size_bytes: 4096 })).toBe(
      "saved",
    );
  });

  it("builds a safe default backup filename from a package id", () => {
    expect(backupDefaultFileName("com.example.app")).toBe("com.example.app.ab");
    expect(backupDefaultFileName("bad/package:name")).toBe(
      "bad_package_name.ab",
    );
  });

  it("formats backup artifact sizes without hiding zero-byte results", () => {
    expect(formatBackupSize(null)).toBeNull();
    expect(formatBackupSize(0)).toBe("0 B");
    expect(formatBackupSize(512)).toBe("512 B");
    expect(formatBackupSize(1536)).toBe("1.5 KiB");
    expect(formatBackupSize(2 * 1024 * 1024)).toBe("2.0 MiB");
  });
});
