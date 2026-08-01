import { describe, expect, it } from "vitest";

import type { WirelessDeviceRisk } from "../lib/tauri";

import {
  requiresWirelessRiskAck,
  unpatchedWirelessDevices,
  wirelessRiskAckKey,
} from "./wirelessRisk";

function device(
  overrides: Partial<WirelessDeviceRisk> & { serial: string },
): WirelessDeviceRisk {
  return {
    model: null,
    security_patch: null,
    risk: "unknown",
    ...overrides,
  };
}

const unpatched = device({
  serial: "AAA111",
  model: "Pixel 8",
  security_patch: "2026-04-01",
  risk: "auth_bypass_unpatched",
});

describe("wireless debugging risk", () => {
  it("flags only devices proven to predate the fix", () => {
    const flagged = unpatchedWirelessDevices([
      unpatched,
      device({
        serial: "BBB222",
        security_patch: "2026-06-01",
        risk: "patched",
      }),
      // An unreadable patch level is not evidence of exposure.
      device({ serial: "CCC333", risk: "unknown" }),
    ]);
    expect(flagged.map((entry) => entry.serial)).toEqual(["AAA111"]);
    expect(flagged[0]!.label).toBe("Pixel 8");
  });

  it("falls back to the serial when the model is missing or blank", () => {
    const flagged = unpatchedWirelessDevices([
      { ...unpatched, model: "   " },
      device({
        serial: "DDD444",
        risk: "auth_bypass_unpatched",
      }),
    ]);
    expect(flagged.map((entry) => entry.label)).toEqual(["AAA111", "DDD444"]);
  });

  it("needs no acknowledgement when nothing is flagged", () => {
    expect(requiresWirelessRiskAck([], null)).toBe(false);
  });

  it("gates until the exact flagged set is acknowledged", () => {
    const flagged = unpatchedWirelessDevices([unpatched]);
    expect(requiresWirelessRiskAck(flagged, null)).toBe(true);
    expect(requiresWirelessRiskAck(flagged, wirelessRiskAckKey(flagged))).toBe(
      false,
    );
  });

  it("re-arms when a different unpatched device appears", () => {
    const first = unpatchedWirelessDevices([unpatched]);
    const acknowledged = wirelessRiskAckKey(first);
    const second = unpatchedWirelessDevices([
      unpatched,
      device({
        serial: "EEE555",
        security_patch: "2026-01-01",
        risk: "auth_bypass_unpatched",
      }),
    ]);
    expect(requiresWirelessRiskAck(second, acknowledged)).toBe(true);
  });

  it("orders flagged devices stably regardless of enumeration order", () => {
    const later = device({
      serial: "ZZZ999",
      risk: "auth_bypass_unpatched",
    });
    expect(
      unpatchedWirelessDevices([later, unpatched]).map((e) => e.serial),
    ).toEqual(
      unpatchedWirelessDevices([unpatched, later]).map((e) => e.serial),
    );
  });
});
