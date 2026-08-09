import { describe, expect, it } from "vitest";

import {
  directoryMutationAllowed,
  mutationBlockReasonForDirectory,
  mutationBlockReasonForEntry,
} from "./filePermissions";

describe("file permission gating", () => {
  it("blocks a directory with no write or execute bit", () => {
    expect(directoryMutationAllowed("dr-xr-xr-x")).toBe(false);
    expect(mutationBlockReasonForDirectory("dr-xr-xr-x")).toBe("permissions");
  });

  it("leaves an unparseable OEM mode enabled", () => {
    expect(directoryMutationAllowed("?")).toBeNull();
    expect(mutationBlockReasonForDirectory("OEM mode")).toBeNull();
  });

  it("allows a writable searchable directory and protects exact roots", () => {
    expect(directoryMutationAllowed("drwxrwx---")).toBe(true);
    expect(mutationBlockReasonForEntry("/system", "drwxrwx---")).toBe(
      "protected",
    );
    expect(mutationBlockReasonForEntry("/system/app", "drwxrwx---")).toBe(null);
  });
});
