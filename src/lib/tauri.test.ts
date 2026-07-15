import { describe, expect, it } from "vitest";

import { inTauri, normalizeWirelessFailure, summarizeState } from "./tauri";

describe("summarizeState", () => {
  it("formats known snake-case states for display", () => {
    expect(summarizeState("no_permissions")).toBe("no permissions");
    expect(summarizeState("unauthorized")).toBe("unauthorized");
  });

  it("handles current and legacy serialized Other variants", () => {
    expect(summarizeState({ other: "rescue" })).toBe("other (rescue)");
    expect(summarizeState({ Other: "legacy" })).toBe("other (legacy)");
  });
});

describe("inTauri", () => {
  it("is false in a plain test runtime", () => {
    expect(inTauri()).toBe(false);
  });
});

describe("normalizeWirelessFailure", () => {
  it("preserves privacy-bounded wireless diagnostics from Tauri", () => {
    const failure = normalizeWirelessFailure({
      code: "wireless_adb_failed",
      message: "adb exited with code 1",
      hint_code: "vpn_interference_likely",
      diagnostics: {
        platform_tools_version: "37.0.0",
        mdns_enabled: true,
        mdns_backend: "LIBADBMDNS",
        mdns_check_succeeded: true,
        active_vpn_interfaces: 1,
        endpoint_kind: "local_name",
        adb_error_kind: "adb_exit",
      },
    });

    expect(failure.message).toBe("adb exited with code 1");
    expect(failure.hintCode).toBe("vpn_interference_likely");
    expect(failure.diagnostics).toMatchObject({
      active_vpn_interfaces: 1,
      endpoint_kind: "local_name",
    });
  });

  it("falls back safely for legacy string errors", () => {
    const failure = normalizeWirelessFailure("connection refused");

    expect(failure.message).toBe("connection refused");
    expect(failure.hintCode).toBeNull();
    expect(failure.diagnostics).toBeNull();
  });
});
