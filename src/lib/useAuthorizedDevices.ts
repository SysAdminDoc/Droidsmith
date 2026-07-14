import { useMemo } from "react";

import { useDeviceStore } from "./deviceStore";
import { deviceTarget, type Device, type DeviceTarget } from "./tauri";

export function useAuthorizedDevices() {
  const devicesState = useDeviceStore((store) => store.devicesState);
  const authorizedDevices = useMemo(
    () =>
      devicesState.kind === "ok"
        ? devicesState.value.devices.filter(
            (device) =>
              typeof device.state === "string" && device.state === "device",
          )
        : [],
    [devicesState],
  );
  return { devicesState, authorizedDevices };
}

/**
 * Keep an exact live target when possible. When ADB assigns a new transport
 * after a reconnect, rebind only when the serial is unique; otherwise require
 * an explicit user choice.
 */
export function resolveAuthorizedTarget(
  previous: DeviceTarget | null,
  devices: Device[],
): DeviceTarget | null {
  if (previous) {
    const exact = devices.find(
      (device) =>
        device.transport_id === previous.transport_id &&
        device.connection_generation === previous.connection_generation,
    );
    if (exact) {
      const refreshed = deviceTarget(exact);
      return sameDeviceTarget(previous, refreshed) ? previous : refreshed;
    }

    const sameSerial = devices.filter(
      (device) => device.serial === previous.serial,
    );
    if (sameSerial.length === 1) return deviceTarget(sameSerial[0]!);
  }

  return devices.length === 1 ? deviceTarget(devices[0]!) : null;
}

export function sameDeviceTarget(
  left: DeviceTarget | null,
  right: DeviceTarget | null,
): boolean {
  if (left === right) return true;
  if (!left || !right) return false;
  return (
    left.serial === right.serial &&
    left.transport_id === right.transport_id &&
    left.connection_generation === right.connection_generation &&
    left.model === right.model &&
    left.product === right.product &&
    left.device === right.device &&
    left.build_fingerprint === right.build_fingerprint
  );
}
