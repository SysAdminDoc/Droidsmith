import { describe, expect, it } from "vitest";

import { loadLogcatLibrary, saveLogcatQueries } from "./logcatQueries";
import type { WorkingQuery } from "../routes/logcatQueries";

// These tests run in the node/browser-fallback path (no Tauri IPC), exercising
// the localStorage-backed persistence layer with an injected storage.

function memoryStorage(initial: Record<string, string> = {}) {
  const values = new Map(Object.entries(initial));
  return {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => void values.set(key, value),
    removeItem: (key: string) => void values.delete(key),
    _dump: () => Object.fromEntries(values),
  };
}

function query(id: string, name = `Query ${id}`): WorkingQuery {
  return {
    id,
    name,
    tagFilter: "",
    messageFilter: "",
    pidFilter: "",
    packageFilter: "",
    processFilter: "",
    minLevel: "I",
    maxAgeSeconds: null,
    useRegex: false,
    negateTag: false,
    negateMessage: false,
    negatePid: false,
    negatePackage: false,
    negateProcess: false,
  };
}

describe("logcat query persistence (browser fallback)", () => {
  it("returns empty scopes when storage is empty", async () => {
    const storage = memoryStorage();
    const library = await loadLogcatLibrary("device-a", storage);
    expect(library.global).toEqual([]);
    expect(library.device).toEqual([]);
  });

  it("round-trips saved global and device queries independently", async () => {
    const storage = memoryStorage();
    await saveLogcatQueries("global", null, [query("g1")], storage);
    await saveLogcatQueries("device", "device-a", [query("d1")], storage);

    const forA = await loadLogcatLibrary("device-a", storage);
    expect(forA.global.map((q) => q.id)).toEqual(["g1"]);
    expect(forA.device.map((q) => q.id)).toEqual(["d1"]);

    // A different device never sees another device's queries.
    const forB = await loadLogcatLibrary("device-b", storage);
    expect(forB.global.map((q) => q.id)).toEqual(["g1"]);
    expect(forB.device).toEqual([]);
  });

  it("clears a scope when saved with an empty list", async () => {
    const storage = memoryStorage();
    await saveLogcatQueries("global", null, [query("g1")], storage);
    await saveLogcatQueries("global", null, [], storage);
    const library = await loadLogcatLibrary(null, storage);
    expect(library.global).toEqual([]);
  });

  it("ignores corrupt or non-array persisted values", async () => {
    const corrupt = memoryStorage({
      "droidsmith.logcat.queries.global": "{not json",
    });
    expect((await loadLogcatLibrary(null, corrupt)).global).toEqual([]);

    const notArray = memoryStorage({
      "droidsmith.logcat.queries.global": JSON.stringify({ id: "x" }),
    });
    expect((await loadLogcatLibrary(null, notArray)).global).toEqual([]);
  });

  it("rejects an invalid query before persisting", async () => {
    const storage = memoryStorage();
    const invalid = { ...query("bad"), name: "" };
    await expect(
      saveLogcatQueries("global", null, [invalid], storage),
    ).rejects.toThrow(/Invalid Logcat query/);
    expect(storage._dump()).toEqual({});
  });
});
