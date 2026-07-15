import { describe, expect, it } from "vitest";

import {
  deviceTarget,
  requiresTransportOverride,
  withTransportOverride,
  type Device,
  type DeviceTarget,
  type DeviceTransportKind,
} from "./tauri";
import { resolveAuthorizedTarget } from "./useAuthorizedDevices";

function device(
  serial: string,
  transportId: number,
  generation: number,
  transportKind: DeviceTransportKind = "usb",
): Device {
  return {
    serial,
    transport_id: transportId,
    connection_generation: generation,
    transport_kind: transportKind,
    state: "device",
    model: null,
    product: null,
    device: null,
    build_fingerprint: null,
    wireless: false,
  };
}

describe("resolveAuthorizedTarget", () => {
  it("rebinds a reconnected serial to its new immutable generation", () => {
    const previous: DeviceTarget = {
      ...device("serial-a", 3, 7),
      untrusted_transport_override: false,
    };
    const target = resolveAuthorizedTarget(previous, [
      device("serial-a", 9, 8),
    ]);
    expect(target?.transport_id).toBe(9);
    expect(target?.connection_generation).toBe(8);
  });

  it("clears an ambiguous or disconnected selection", () => {
    const previous: DeviceTarget = {
      ...device("serial-a", 3, 7),
      untrusted_transport_override: false,
    };
    const devices = [device("serial-b", 4, 1), device("serial-c", 5, 1)];
    expect(resolveAuthorizedTarget(previous, devices)).toBeNull();
  });

  it("preserves the target object for an unchanged live connection", () => {
    const live = device("serial-a", 3, 7);
    const previous: DeviceTarget = {
      ...live,
      untrusted_transport_override: false,
    };
    expect(resolveAuthorizedTarget(previous, [live])).toBe(previous);
  });

  it("fails closed when live transport provenance changes", () => {
    const previous = deviceTarget(device("serial-a", 3, 7, "tls_wifi"));
    const resolved = resolveAuthorizedTarget(previous, [
      device("serial-a", 3, 7, "unknown_tcp"),
    ]);
    expect(resolved).not.toBe(previous);
    expect(resolved?.transport_kind).toBe("unknown_tcp");
    expect(resolved?.untrusted_transport_override).toBe(false);
  });

  it("only carries acknowledgement on transports that require it", () => {
    const unknown = deviceTarget(
      device("wifi.local:5555", 4, 8, "unknown_tcp"),
    );
    const tls = deviceTarget(device("wifi.local:38899", 5, 9, "tls_wifi"));
    expect(requiresTransportOverride(unknown)).toBe(true);
    expect(
      withTransportOverride(unknown, true).untrusted_transport_override,
    ).toBe(true);
    expect(withTransportOverride(tls, true).untrusted_transport_override).toBe(
      false,
    );
  });
});
