import { describe, expect, it } from "vitest";

import type { Device, DeviceTarget } from "./tauri";
import { resolveAuthorizedTarget } from "./useAuthorizedDevices";

function device(
  serial: string,
  transportId: number,
  generation: number,
): Device {
  return {
    serial,
    transport_id: transportId,
    connection_generation: generation,
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
    };
    const target = resolveAuthorizedTarget(previous, [
      device("serial-a", 9, 8),
    ]);
    expect(target?.transport_id).toBe(9);
    expect(target?.connection_generation).toBe(8);
  });

  it("clears an ambiguous or disconnected selection", () => {
    const previous: DeviceTarget = { ...device("serial-a", 3, 7) };
    const devices = [device("serial-b", 4, 1), device("serial-c", 5, 1)];
    expect(resolveAuthorizedTarget(previous, devices)).toBeNull();
  });

  it("preserves the target object for an unchanged live connection", () => {
    const live = device("serial-a", 3, 7);
    const previous: DeviceTarget = { ...live };
    expect(resolveAuthorizedTarget(previous, [live])).toBe(previous);
  });
});
