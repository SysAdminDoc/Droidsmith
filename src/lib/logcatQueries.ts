import {
  callListLogcatQueries,
  callSaveLogcatQueries,
  inTauri,
  type LogcatQueryScope,
} from "./tauri";
import {
  MAX_LOGCAT_QUERIES,
  normalizeQuery,
  toQueryDto,
  validateQuery,
  type WorkingQuery,
} from "../routes/logcatQueries";

export type LogcatLibrary = {
  global: WorkingQuery[];
  device: WorkingQuery[];
};

type BrowserStorage = Pick<Storage, "getItem" | "setItem" | "removeItem">;

const GLOBAL_KEY = "droidsmith.logcat.queries.global";
const DEVICE_PREFIX = "droidsmith.logcat.queries.device.";

export async function loadLogcatLibrary(
  deviceIdentity: string | null,
  storage: BrowserStorage | null = browserStorage(),
): Promise<LogcatLibrary> {
  if (!inTauri()) {
    return {
      global: readBrowserScope(storage, GLOBAL_KEY),
      device: deviceIdentity
        ? readBrowserScope(storage, DEVICE_PREFIX + deviceIdentity)
        : [],
    };
  }
  const library = await callListLogcatQueries(deviceIdentity);
  return {
    global: library.global.map(normalizeQuery),
    device: library.device.map(normalizeQuery),
  };
}

export async function saveLogcatQueries(
  scope: LogcatQueryScope,
  deviceIdentity: string | null,
  queries: WorkingQuery[],
  storage: BrowserStorage | null = browserStorage(),
): Promise<LogcatLibrary> {
  const invalid = queries.find((query) => validateQuery(query) !== null);
  if (invalid) {
    throw new Error(`Invalid Logcat query: ${invalid.name || invalid.id}`);
  }
  if (queries.length > MAX_LOGCAT_QUERIES) {
    throw new Error(`Keep at most ${MAX_LOGCAT_QUERIES} saved queries.`);
  }
  const dtos = queries.map(toQueryDto);
  if (!inTauri()) {
    const key =
      scope === "device" ? DEVICE_PREFIX + (deviceIdentity ?? "") : GLOBAL_KEY;
    if (dtos.length === 0) storage?.removeItem(key);
    else storage?.setItem(key, JSON.stringify(dtos));
    return loadLogcatLibrary(deviceIdentity, storage);
  }
  const library = await callSaveLogcatQueries(scope, deviceIdentity, dtos);
  return {
    global: library.global.map(normalizeQuery),
    device: library.device.map(normalizeQuery),
  };
}

function readBrowserScope(
  storage: BrowserStorage | null,
  key: string,
): WorkingQuery[] {
  try {
    const raw = storage?.getItem(key);
    if (!raw) return [];
    const parsed: unknown = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    return parsed
      .filter((entry): entry is Record<string, unknown> =>
        Boolean(entry && typeof entry === "object"),
      )
      .map(normalizeQuery)
      .filter((query) => validateQuery(query) === null);
  } catch {
    return [];
  }
}

function browserStorage(): BrowserStorage | null {
  try {
    return typeof window === "undefined" ? null : window.localStorage;
  } catch {
    return null;
  }
}
