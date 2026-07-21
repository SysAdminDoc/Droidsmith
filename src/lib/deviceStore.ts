import { useSyncExternalStore } from "react";

import {
  errorMessage,
  callCancelOperation,
  callListDevices,
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
  let fallbackInFlight = false;
  setDeviceStoreState({
    devicesState: { kind: "loading" },
    watching: true,
  });

  const scanOnce = async (watchError: string, watcherStillActive: boolean) => {
    if (fallbackInFlight) return;
    fallbackInFlight = true;
    try {
      const value = await callListDevices();
      if (
        activeOperationId !== operationId ||
        lifecycleGeneration !== generation
      )
        return;
      setDeviceStoreState({
        devicesState: { kind: "ok", value },
        observedAt: new Date().toISOString(),
        watching: watcherStillActive,
      });
    } catch (fallbackError) {
      if (
        activeOperationId !== operationId ||
        lifecycleGeneration !== generation
      )
        return;
      const fallbackMessage =
        fallbackError instanceof Error
          ? fallbackError.message
          : String(fallbackError);
      setDeviceStoreState({
        devicesState: {
          kind: "error",
          message: `${watchError} ${fallbackMessage}`,
        },
        watching: watcherStillActive,
      });
    } finally {
      fallbackInFlight = false;
    }
  };

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
      void scanOnce(event.message, true);
    }
  };

  void callWatchDevices({ operationId, onEvent })
    .catch(async (error) => {
      if (
        activeOperationId === operationId &&
        lifecycleGeneration === generation
      ) {
        await scanOnce(errorMessage(error), false);
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

/** Test-only accessor for the module-level store singleton. */
export function __getDeviceStoreForTests(): DeviceStore {
  return store;
}

/** Test-only reset so each test starts from a clean lifecycle. */
export function __resetDeviceStoreForTests() {
  activeOperationId = null;
  lifecycleGeneration += 1;
  store = {
    devicesState: { kind: "loading" },
    health: null,
    observedAt: null,
    watching: false,
  };
  listeners.clear();
}
