import { useSyncExternalStore } from "react";

import {
  callCancelOperation,
  callWatchDevices,
  inTauri,
  newOperationId,
  type AdbHealth,
  type DeviceLifecycleEvent,
  type ListDevicesResult,
} from "./tauri";

export type SharedDevicesState =
  | { kind: "loading" }
  | { kind: "no_tauri" }
  | { kind: "ok"; value: ListDevicesResult }
  | { kind: "error"; message: string };

type DeviceStore = {
  devicesState: SharedDevicesState;
  health: AdbHealth | null;
  observedAt: string | null;
  watching: boolean;
};

let store: DeviceStore = {
  devicesState: { kind: "loading" },
  health: null,
  observedAt: null,
  watching: false,
};
const listeners = new Set<() => void>();

function setDeviceStoreState(update: Partial<DeviceStore>) {
  store = { ...store, ...update };
  for (const listener of listeners) listener();
}

export function useDeviceStore<T>(selector: (state: DeviceStore) => T): T {
  return useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    () => selector(store),
    () => selector(store),
  );
}

let activeOperationId: string | null = null;
let lifecycleGeneration = 0;

export function startDeviceLifecycle() {
  if (activeOperationId) return;
  if (!inTauri()) {
    setDeviceStoreState({
      devicesState: { kind: "no_tauri" },
      health: null,
      observedAt: null,
      watching: false,
    });
    return;
  }

  const operationId = newOperationId("device-watch");
  const generation = lifecycleGeneration + 1;
  lifecycleGeneration = generation;
  activeOperationId = operationId;
  setDeviceStoreState({
    devicesState: { kind: "loading" },
    watching: true,
  });

  const onEvent = (event: DeviceLifecycleEvent) => {
    if (activeOperationId !== operationId || lifecycleGeneration !== generation)
      return;
    if (event.kind === "snapshot") {
      setDeviceStoreState({
        devicesState: { kind: "ok", value: event.result },
        health: event.health,
        observedAt: event.observed_at,
        watching: true,
      });
    } else {
      setDeviceStoreState({
        devicesState: { kind: "error", message: event.message },
        observedAt: event.observed_at,
        watching: true,
      });
    }
  };

  void callWatchDevices({ operationId, onEvent })
    .catch((error) => {
      if (
        activeOperationId === operationId &&
        lifecycleGeneration === generation
      ) {
        setDeviceStoreState({
          devicesState: {
            kind: "error",
            message: error instanceof Error ? error.message : String(error),
          },
          watching: false,
        });
      }
    })
    .finally(() => {
      if (
        activeOperationId === operationId &&
        lifecycleGeneration === generation
      ) {
        activeOperationId = null;
        setDeviceStoreState({ watching: false });
      }
    });
}

export async function stopDeviceLifecycle() {
  const operationId = activeOperationId;
  activeOperationId = null;
  lifecycleGeneration += 1;
  setDeviceStoreState({ watching: false });
  if (operationId) await callCancelOperation(operationId);
}

export async function restartDeviceLifecycle() {
  await stopDeviceLifecycle();
  startDeviceLifecycle();
}
