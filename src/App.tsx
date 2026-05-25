import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

type ResolveSource =
  | "path"
  | "android_home"
  | "android_studio"
  | "homebrew"
  | "distro_package"
  | "bundled"
  | "not_found";

type AdbResolution = {
  path: string | null;
  source: ResolveSource;
  version: string | null;
};

type Heartbeat = {
  version: string;
  os: { family: string; version: string; arch: string };
  tauri_version: string;
  rust_version: string;
  app_data_dir: string | null;
  adb: AdbResolution;
};

export type NavItem = {
  label: string;
  milestone: string;
  description: string;
};

/** Single source of truth for the sidebar. Exported so tests can verify the
 *  list stays aligned with `ROADMAP.md` without duplicating it. */
export const NAV_ITEMS: ReadonlyArray<NavItem> = [
  {
    label: "Devices",
    milestone: "R-012",
    description: "USB + wireless device discovery, hot-plug, multi-device.",
  },
  {
    label: "Apps",
    milestone: "R-020",
    description:
      "Installed apps with real labels and icons, filters, bulk actions.",
  },
  {
    label: "Debloat",
    milestone: "R-033",
    description: "Pick a pack, preview the diff, apply, undo from the journal.",
  },
  {
    label: "Mirror",
    milestone: "R-040",
    description: "scrcpy-driven screen mirror with audio and recording.",
  },
  {
    label: "Console",
    milestone: "R-050",
    description: "Multi-tab adb shell with history and favourites.",
  },
  {
    label: "Logcat",
    milestone: "R-051",
    description: "Live tail with tag / pid / level filters and grep.",
  },
  {
    label: "Fastboot",
    milestone: "R-052",
    description: "Fastboot mode, partition inspector, slot management.",
  },
] as const;

type LoadState =
  | { status: "loading" }
  | { status: "ok"; value: Heartbeat }
  | { status: "error"; error: string };

export default function App() {
  const [state, setState] = useState<LoadState>({ status: "loading" });
  const [active, setActive] = useState<string>(NAV_ITEMS[0].label);

  const loadHeartbeat = useCallback(async () => {
    setState({ status: "loading" });
    try {
      const value = await invoke<Heartbeat>("heartbeat");
      setState({ status: "ok", value });
    } catch (e) {
      setState({
        status: "error",
        error: e instanceof Error ? e.message : String(e),
      });
    }
  }, []);

  useEffect(() => {
    void loadHeartbeat();
  }, [loadHeartbeat]);

  const activeItem = NAV_ITEMS.find((i) => i.label === active) ?? NAV_ITEMS[0];

  return (
    <div className="flex h-full">
      <aside className="flex w-56 flex-col border-r border-anvil-800 bg-anvil-950 p-4">
        <h1 className="font-mono text-lg font-semibold tracking-tight">
          Droidsmith
        </h1>
        <p className="mt-1 text-xs text-anvil-300">
          {state.status === "ok" ? `v${state.value.version}` : "v…"}
        </p>
        <nav className="mt-6 space-y-1 text-sm" aria-label="Primary">
          {NAV_ITEMS.map((item) => (
            <NavStub
              key={item.label}
              item={item}
              active={active === item.label}
              onActivate={() => setActive(item.label)}
            />
          ))}
        </nav>
      </aside>
      <main className="flex flex-1 flex-col overflow-auto p-8">
        <header className="mb-6 flex items-baseline justify-between gap-4">
          <h2 className="text-2xl font-semibold">{activeItem.label}</h2>
          <p className="text-xs text-anvil-300">
            Coming in{" "}
            <code className="rounded bg-anvil-800 px-1.5 py-0.5 font-mono">
              {activeItem.milestone}
            </code>
          </p>
        </header>
        <p className="max-w-prose text-sm text-anvil-200">
          {activeItem.description}
        </p>
        <p className="mt-2 max-w-prose text-xs text-anvil-300">
          The shell is here; features land per the{" "}
          <code className="rounded bg-anvil-800 px-1 py-0.5 font-mono text-xs">
            ROADMAP.md
          </code>
          . Until that milestone closes, this pane is a placeholder.
        </p>
        <HeartbeatPanel state={state} onRetry={() => void loadHeartbeat()} />
      </main>
    </div>
  );
}

function NavStub({
  item,
  active,
  onActivate,
}: {
  item: NavItem;
  active: boolean;
  onActivate: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onActivate}
      aria-current={active ? "page" : undefined}
      aria-describedby={`${item.label}-milestone`}
      className={[
        "flex w-full items-center justify-between rounded px-2 py-1.5 text-left transition-colors",
        "focus:outline-none focus-visible:ring-2 focus-visible:ring-anvil-300",
        active
          ? "bg-anvil-800 text-anvil-50"
          : "text-anvil-100 hover:bg-anvil-900",
      ].join(" ")}
    >
      <span>{item.label}</span>
      <span
        id={`${item.label}-milestone`}
        className="font-mono text-[10px] text-anvil-300"
      >
        {item.milestone}
      </span>
    </button>
  );
}

function HeartbeatPanel({
  state,
  onRetry,
}: {
  state: LoadState;
  onRetry: () => void;
}) {
  return (
    <section
      className="mt-8 max-w-lg"
      aria-labelledby="heartbeat-title"
      aria-live="polite"
    >
      <h3
        id="heartbeat-title"
        className="mb-2 text-sm font-semibold text-anvil-50"
      >
        Heartbeat
      </h3>
      <div className="rounded-lg border border-anvil-800 bg-anvil-900 p-4 font-mono text-xs">
        {state.status === "loading" && (
          <div role="status" className="text-anvil-200">
            loading…
          </div>
        )}
        {state.status === "error" && (
          <div className="space-y-2">
            <div role="alert" className="text-red-300">
              Heartbeat failed: {state.error}
            </div>
            <button
              type="button"
              onClick={onRetry}
              className="rounded border border-anvil-700 bg-anvil-800 px-2 py-1 text-anvil-50 hover:bg-anvil-700 focus:outline-none focus-visible:ring-2 focus-visible:ring-anvil-300"
            >
              Retry
            </button>
          </div>
        )}
        {state.status === "ok" && <HeartbeatTable value={state.value} />}
      </div>
    </section>
  );
}

function HeartbeatTable({ value }: { value: Heartbeat }) {
  return (
    <dl className="grid grid-cols-[8rem_1fr] gap-y-1">
      <KV k="droidsmith" v={`v${value.version}`} />
      <KV
        k="os"
        v={`${value.os.family} ${value.os.version} (${value.os.arch})`}
      />
      <KV k="tauri" v={`v${value.tauri_version}`} />
      <KV k="rust msrv" v={value.rust_version} />
      <KV k="app data" v={value.app_data_dir ?? "—"} breakable />
      <KV
        k="adb"
        v={
          value.adb.path
            ? `${value.adb.path}${value.adb.version ? ` — ${value.adb.version}` : ""}`
            : "not detected (bundle landing in R-010)"
        }
        breakable
      />
      <KV k="adb source" v={value.adb.source.replace(/_/g, " ")} />
    </dl>
  );
}

function KV({
  k,
  v,
  breakable,
}: {
  k: string;
  v: string;
  breakable?: boolean;
}) {
  // For paths, insert a zero-width space after each path separator so the
  // browser can break on segment boundaries instead of mid-word.
  const display = breakable ? insertWordBreaks(v) : v;
  return (
    <>
      <dt className="text-anvil-300">{k}</dt>
      <dd className="break-words text-anvil-50">{display}</dd>
    </>
  );
}

function insertWordBreaks(s: string): string {
  return s.replace(/([/\\])/g, "$1​");
}
