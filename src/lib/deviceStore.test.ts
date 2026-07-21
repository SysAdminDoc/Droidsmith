import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// Control the tauri boundary so the store can be driven deterministically.
const mocks = vi.hoisted(() => ({
  inTauri: vi.fn(() => true),
  callWatchDevices: vi.fn(),
  callListDevices: vi.fn(),
  callCancelOperation: vi.fn(async () => {}),
}));

vi.mock("./tauri", () => ({
  inTauri: mocks.inTauri,
  callWatchDevices: mocks.callWatchDevices,
  callListDevices: mocks.callListDevices,
  callCancelOperation: mocks.callCancelOperation,
  errorMessage: (error: unknown) =>
    error instanceof Error ? error.message : String(error),
  newOperationId: (prefix: string) => `${prefix}-test`,
}));

import {
  __getDeviceStoreForTests,
  __resetDeviceStoreForTests,
  startDeviceLifecycle,
  stopDeviceLifecycle,
} from "./deviceStore";

const OK_RESULT = {
  adb_resolved: true,
  adb_path: "/usr/bin/adb",
  devices: [],
};

const HEALTH = { kind: "ok" } as const;

describe("deviceStore lifecycle", () => {
  beforeEach(() => {
    __resetDeviceStoreForTests();
    mocks.inTauri.mockReturnValue(true);
    mocks.callWatchDevices.mockReset();
    mocks.callListDevices.mockReset();
    mocks.callCancelOperation.mockClear();
  });

  afterEach(async () => {
    await stopDeviceLifecycle();
  });

  it("reports no_tauri without watching when not in the desktop shell", () => {
    mocks.inTauri.mockReturnValue(false);
    startDeviceLifecycle();
    const state = __getDeviceStoreForTests();
    expect(state.devicesState.kind).toBe("no_tauri");
    expect(state.watching).toBe(false);
    expect(mocks.callWatchDevices).not.toHaveBeenCalled();
  });

  it("applies watcher snapshot and error events to the store", async () => {
    let emit: (event: unknown) => void = () => {};
    mocks.callWatchDevices.mockImplementation(({ onEvent }) => {
      emit = onEvent;
      return new Promise(() => {}); // stays active for the test
    });
    mocks.callListDevices.mockResolvedValue({
      ...OK_RESULT,
      devices: [{ serial: "fallback" }],
    });

    startDeviceLifecycle();
    expect(__getDeviceStoreForTests().devicesState.kind).toBe("loading");
    expect(__getDeviceStoreForTests().watching).toBe(true);

    emit({
      kind: "snapshot",
      result: OK_RESULT,
      health: HEALTH,
      observed_at: "2026-07-20T00:00:00Z",
    });
    let state = __getDeviceStoreForTests();
    expect(state.devicesState.kind).toBe("ok");
    expect(state.health).toEqual(HEALTH);
    expect(state.observedAt).toBe("2026-07-20T00:00:00Z");

    // An error event surfaces the message and triggers a fallback scan.
    emit({
      kind: "error",
      message: "watcher fell over",
      observed_at: "2026-07-20T00:00:01Z",
    });
    expect(__getDeviceStoreForTests().devicesState.kind).toBe("error");
    // Let the fallback callListDevices resolve.
    await Promise.resolve();
    await Promise.resolve();
    state = __getDeviceStoreForTests();
    expect(state.devicesState.kind).toBe("ok");
    expect(mocks.callListDevices).toHaveBeenCalledTimes(1);
  });

  it("ignores events from a superseded lifecycle generation", async () => {
    let emitOld: (event: unknown) => void = () => {};
    mocks.callWatchDevices.mockImplementationOnce(({ onEvent }) => {
      emitOld = onEvent;
      return new Promise(() => {});
    });
    startDeviceLifecycle();
    await stopDeviceLifecycle();

    // A late event from the stopped watcher must not mutate the store.
    emitOld({
      kind: "snapshot",
      result: OK_RESULT,
      health: HEALTH,
      observed_at: "stale",
    });
    expect(__getDeviceStoreForTests().devicesState.kind).not.toBe("ok");
    expect(mocks.callCancelOperation).toHaveBeenCalled();
  });
});
