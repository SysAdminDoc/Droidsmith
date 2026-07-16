import type {
  LogcatLevel,
  LogcatQuery,
  LogcatQueryScope,
} from "../lib/bindings";

export type { LogcatLevel, LogcatQuery, LogcatQueryScope };

export const LOGCAT_LEVELS: LogcatLevel[] = ["V", "D", "I", "W", "E", "F"];

export const MAX_LOGCAT_QUERIES = 64;
export const MAX_LOGCAT_FILTER_LENGTH = 256;
export const MAX_LOGCAT_NAME_LENGTH = 80;
export const MAX_LOGCAT_AGE_SECONDS = 30 * 24 * 60 * 60;
/** Bound the session-only "recently applied" list; only ids are retained. */
export const MAX_LOGCAT_HISTORY = 8;

/** A parsed logcat row. Only used in memory — never persisted. */
export type LogLine = {
  raw: string;
  level: string;
  tag: string;
  pid: string;
  message: string;
  timeMs: number | null;
};

/** A fully-populated query used in the UI; the IPC DTO leaves fields optional. */
export type WorkingQuery = {
  id: string;
  name: string;
  tagFilter: string;
  messageFilter: string;
  pidFilter: string;
  packageFilter: string;
  processFilter: string;
  minLevel: LogcatLevel;
  maxAgeSeconds: number | null;
  useRegex: boolean;
  negateTag: boolean;
  negateMessage: boolean;
  negatePid: boolean;
  negatePackage: boolean;
  negateProcess: boolean;
};

export const DEFAULT_QUERY: WorkingQuery = {
  id: "",
  name: "",
  tagFilter: "",
  messageFilter: "",
  pidFilter: "",
  packageFilter: "",
  processFilter: "",
  minLevel: "V",
  maxAgeSeconds: null,
  useRegex: false,
  negateTag: false,
  negateMessage: false,
  negatePid: false,
  negatePackage: false,
  negateProcess: false,
};

/**
 * Read-only presets that approximate Android Studio's `is:crash` and
 * `is:stacktrace` special filters. Their regexes stay inside the linear-time
 * subset so they can never trigger catastrophic backtracking.
 */
export const BUILTIN_QUERIES: readonly WorkingQuery[] = Object.freeze([
  {
    ...DEFAULT_QUERY,
    id: "builtin-crash",
    name: "Crashes & ANRs",
    messageFilter: "FATAL EXCEPTION|ANR in |Fatal signal |signal [0-9]+ \\(SIG",
    minLevel: "E",
    useRegex: true,
  },
  {
    ...DEFAULT_QUERY,
    id: "builtin-stacktrace",
    name: "Stack traces",
    messageFilter: "^\\s*at [\\w$.]+\\(|^Caused by:|^\\s*\\.\\.\\. [0-9]+ more",
    minLevel: "V",
    useRegex: true,
  },
]);

const BUILTIN_IDS = new Set(BUILTIN_QUERIES.map((query) => query.id));

export function isBuiltinQuery(id: string): boolean {
  return BUILTIN_IDS.has(id);
}

let idCounter = 0;

export function newQueryId(): string {
  idCounter += 1;
  const random = Math.floor(Math.random() * 1e9).toString(36);
  return `q-${Date.now().toString(36)}-${idCounter.toString(36)}-${random}`;
}

/**
 * Reject regex constructs a backtracking engine can evaluate in super-linear
 * time. Mirrors the Rust `validate_linear_regex` guard so the renderer and the
 * durable store agree on the supported subset.
 */
export function regexError(
  pattern: string,
): "backreference" | "lookaround" | "nestedQuantifier" | "syntax" | null {
  if (/\\[0-9]/u.test(pattern) || /\\k/u.test(pattern)) return "backreference";
  if (/\(\?[=!]/u.test(pattern) || /\(\?<[=!]/u.test(pattern))
    return "lookaround";
  if (/\)[*+]/u.test(pattern) || /\)\{/u.test(pattern))
    return "nestedQuantifier";
  try {
    void new RegExp(pattern, "u");
  } catch {
    return "syntax";
  }
  return null;
}

export type QueryFieldError = { field: keyof WorkingQuery; code: string };

/** Field-level validation used to guide the user before a save is attempted. */
export function validateQuery(query: WorkingQuery): QueryFieldError | null {
  if (
    query.name.trim().length === 0 ||
    query.name.length > MAX_LOGCAT_NAME_LENGTH
  ) {
    return { field: "name", code: "name" };
  }
  for (const field of [
    "tagFilter",
    "messageFilter",
    "packageFilter",
    "processFilter",
  ] as const) {
    const value = query[field];
    if (value.length > MAX_LOGCAT_FILTER_LENGTH) {
      return { field, code: "tooLong" };
    }
    if (query.useRegex && value.length > 0 && regexError(value)) {
      return { field, code: regexError(value) ?? "syntax" };
    }
  }
  if (query.pidFilter.length > 0 && !/^[0-9]{1,7}$/u.test(query.pidFilter)) {
    return { field: "pidFilter", code: "pid" };
  }
  if (
    query.maxAgeSeconds !== null &&
    (!Number.isSafeInteger(query.maxAgeSeconds) ||
      query.maxAgeSeconds < 1 ||
      query.maxAgeSeconds > MAX_LOGCAT_AGE_SECONDS)
  ) {
    return { field: "maxAgeSeconds", code: "age" };
  }
  return null;
}

/** Fill a partial (imported or IPC) record with concrete defaults. */
export function normalizeQuery(value: Partial<LogcatQuery>): WorkingQuery {
  return {
    id: typeof value.id === "string" && value.id ? value.id : newQueryId(),
    name: typeof value.name === "string" ? value.name : "",
    tagFilter: typeof value.tagFilter === "string" ? value.tagFilter : "",
    messageFilter:
      typeof value.messageFilter === "string" ? value.messageFilter : "",
    pidFilter: typeof value.pidFilter === "string" ? value.pidFilter : "",
    packageFilter:
      typeof value.packageFilter === "string" ? value.packageFilter : "",
    processFilter:
      typeof value.processFilter === "string" ? value.processFilter : "",
    minLevel: LOGCAT_LEVELS.includes(value.minLevel as LogcatLevel)
      ? (value.minLevel as LogcatLevel)
      : "V",
    maxAgeSeconds:
      typeof value.maxAgeSeconds === "number" && value.maxAgeSeconds > 0
        ? value.maxAgeSeconds
        : null,
    useRegex: value.useRegex === true,
    negateTag: value.negateTag === true,
    negateMessage: value.negateMessage === true,
    negatePid: value.negatePid === true,
    negatePackage: value.negatePackage === true,
    negateProcess: value.negateProcess === true,
  };
}

/** Convert a working query into the IPC DTO the backend persists. */
export function toQueryDto(query: WorkingQuery): LogcatQuery {
  return {
    id: query.id,
    name: query.name.trim(),
    tagFilter: query.tagFilter,
    messageFilter: query.messageFilter,
    pidFilter: query.pidFilter,
    packageFilter: query.packageFilter,
    processFilter: query.processFilter,
    minLevel: query.minLevel,
    maxAgeSeconds: query.maxAgeSeconds,
    useRegex: query.useRegex,
    negateTag: query.negateTag,
    negateMessage: query.negateMessage,
    negatePid: query.negatePid,
    negatePackage: query.negatePackage,
    negateProcess: query.negateProcess,
  };
}

function textMatches(
  haystack: string,
  needle: string,
  useRegex: boolean,
): boolean {
  if (needle.length === 0) return true;
  if (useRegex) {
    if (regexError(needle)) return false;
    try {
      return new RegExp(needle, "u").test(haystack);
    } catch {
      return false;
    }
  }
  return haystack.toLowerCase().includes(needle.toLowerCase());
}

/**
 * Apply a query to one parsed line. `nowMs` anchors the age comparison;
 * `processNames` maps a PID to its process name (from `ps`) so package/process
 * filters can resolve the line's owner. A line whose PID is not in the map is
 * surfaced (never hidden by a package/process filter) so a stale or incomplete
 * process snapshot cannot silently drop log output.
 */
export function matchesLine(
  line: LogLine,
  query: WorkingQuery,
  nowMs: number,
  processNames: ReadonlyMap<string, string> = EMPTY_PROCESS_MAP,
): boolean {
  const lineLevel = LOGCAT_LEVELS.indexOf(line.level as LogcatLevel);
  const minLevel = LOGCAT_LEVELS.indexOf(query.minLevel);
  if (lineLevel >= 0 && minLevel >= 0 && lineLevel < minLevel) return false;

  if (query.tagFilter.length > 0) {
    const hit = textMatches(line.tag, query.tagFilter, query.useRegex);
    if (hit === query.negateTag) return false;
  }
  if (query.messageFilter.length > 0) {
    const hit = textMatches(line.message, query.messageFilter, query.useRegex);
    if (hit === query.negateMessage) return false;
  }
  if (query.pidFilter.length > 0) {
    const hit = line.pid === query.pidFilter;
    if (hit === query.negatePid) return false;
  }
  if (query.processFilter.length > 0) {
    const processName = processNames.get(line.pid);
    if (processName !== undefined) {
      const hit = textMatches(processName, query.processFilter, query.useRegex);
      if (hit === query.negateProcess) return false;
    }
  }
  if (query.packageFilter.length > 0) {
    const processName = processNames.get(line.pid);
    if (processName !== undefined) {
      // An app process name is the package plus an optional ":component" suffix.
      const packageName = processName.split(":")[0] ?? processName;
      const hit = textMatches(packageName, query.packageFilter, query.useRegex);
      if (hit === query.negatePackage) return false;
    }
  }
  if (query.maxAgeSeconds !== null && line.timeMs !== null) {
    if (nowMs - line.timeMs > query.maxAgeSeconds * 1000) return false;
  }
  return true;
}

const EMPTY_PROCESS_MAP: ReadonlyMap<string, string> = new Map();

const THREADTIME =
  /^(\d{2})-(\d{2}) (\d{2}):(\d{2}):(\d{2})\.(\d{3})\s+(\d+)\s+\d+\s+([VDIWEF])\s+(.*?):\s?(.*)$/u;
const BRIEF = /^([VDIWEF])\/([^(]+)\(\s*(\d+)\):\s*(.*)$/u;

/** Parse an adb `threadtime` (preferred) or `brief` logcat line. */
export function parseLogcatLine(raw: string, nowMs = Date.now()): LogLine {
  const threaded = raw.match(THREADTIME);
  if (threaded) {
    const [, mm, dd, hh, min, ss, ms, pid, level, tag, message] = threaded;
    return {
      raw,
      level: level!,
      tag: tag!.trim(),
      pid: pid!,
      message: message!,
      timeMs: threadtimeToMs(mm!, dd!, hh!, min!, ss!, ms!, nowMs),
    };
  }
  const brief = raw.match(BRIEF);
  if (brief) {
    return {
      raw,
      level: brief[1]!,
      tag: brief[2]!.trim(),
      pid: brief[3]!,
      message: brief[4]!,
      timeMs: null,
    };
  }
  return { raw, level: "", tag: "", pid: "", message: raw, timeMs: null };
}

function threadtimeToMs(
  mm: string,
  dd: string,
  hh: string,
  min: string,
  ss: string,
  ms: string,
  nowMs: number,
): number | null {
  const reference = new Date(nowMs);
  const candidate = new Date(
    reference.getFullYear(),
    Number(mm) - 1,
    Number(dd),
    Number(hh),
    Number(min),
    Number(ss),
    Number(ms),
  );
  const value = candidate.getTime();
  if (Number.isNaN(value)) return null;
  // Logcat timestamps omit the year; a line dated after "now" belongs to the
  // previous calendar year (a December tail read in early January).
  if (value > nowMs + 24 * 60 * 60 * 1000) {
    candidate.setFullYear(reference.getFullYear() - 1);
    return candidate.getTime();
  }
  return value;
}

/** Serialize a library for the plain-text export/import affordance. */
export function serializeQueries(queries: WorkingQuery[]): string {
  return `${JSON.stringify(
    { version: "1", queries: queries.map(toQueryDto) },
    null,
    2,
  )}\n`;
}

export type ParsedImport =
  | { ok: true; queries: WorkingQuery[] }
  | { ok: false; error: "format" | "empty" | "tooMany" };

/** Parse and re-validate an imported library; the backend re-checks on save. */
export function parseImportedQueries(text: string): ParsedImport {
  let parsed: unknown;
  try {
    parsed = JSON.parse(text);
  } catch {
    return { ok: false, error: "format" };
  }
  const raw =
    parsed && typeof parsed === "object" && "queries" in parsed
      ? (parsed as { queries: unknown }).queries
      : parsed;
  if (!Array.isArray(raw)) return { ok: false, error: "format" };
  if (raw.length === 0) return { ok: false, error: "empty" };
  if (raw.length > MAX_LOGCAT_QUERIES) return { ok: false, error: "tooMany" };
  const seen = new Set<string>();
  const queries: WorkingQuery[] = [];
  for (const entry of raw) {
    if (!entry || typeof entry !== "object")
      return { ok: false, error: "format" };
    const query = normalizeQuery(entry as Partial<LogcatQuery>);
    if (validateQuery(query)) return { ok: false, error: "format" };
    // Imported ids are regenerated on collision so a merge never overwrites.
    if (seen.has(query.id)) query.id = newQueryId();
    seen.add(query.id);
    queries.push(query);
  }
  return { ok: true, queries };
}
