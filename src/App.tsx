import { useCallback, useEffect, useState } from "react";
import type { ReactNode } from "react";

import { callHeartbeat, type Heartbeat } from "./lib/tauri";

import DevicesRoute from "./routes/Devices";
import {
  AppsRoute,
  ConsoleRoute,
  DebloatRoute,
  FastbootRoute,
  LogcatRoute,
  MirrorRoute,
} from "./routes/placeholders";

export type NavItem = {
  label: string;
  milestone: string;
  description: string;
  render: () => ReactNode;
};

/** Single source of truth for the sidebar. Exported so tests can verify
 *  the list stays aligned with `ROADMAP.md` without duplicating it. */
export const NAV_ITEMS: ReadonlyArray<NavItem> = [
  {
    label: "Devices",
    milestone: "R-012",
    description: "USB + wireless device discovery, hot-plug, multi-device.",
    render: () => <DevicesRoute />,
  },
  {
    label: "Apps",
    milestone: "R-020",
    description:
      "Installed apps with real labels and icons, filters, bulk actions.",
    render: () => <AppsRoute />,
  },
  {
    label: "Debloat",
    milestone: "R-033",
    description: "Pick a pack, preview the diff, apply, undo from the journal.",
    render: () => <DebloatRoute />,
  },
  {
    label: "Mirror",
    milestone: "R-040",
    description: "scrcpy-driven screen mirror with audio and recording.",
    render: () => <MirrorRoute />,
  },
  {
    label: "Console",
    milestone: "R-050",
    description: "Multi-tab adb shell with history and favourites.",
    render: () => <ConsoleRoute />,
  },
  {
    label: "Logcat",
    milestone: "R-051",
    description: "Live tail with tag / pid / level filters and grep.",
    render: () => <LogcatRoute />,
  },
  {
    label: "Fastboot",
    milestone: "R-052",
    description: "Fastboot mode, partition inspector, slot management.",
    render: () => <FastbootRoute />,
  },
] as const;

type LoadState =
  | { status: "loading" }
  | { status: "ok"; value: Heartbeat }
  | { status: "error"; error: string };

export default function App() {
  const [hb, setHb] = useState<LoadState>({ status: "loading" });
  const [active, setActive] = useState<string>(NAV_ITEMS[0].label);

  const loadHeartbeat = useCallback(async () => {
    setHb({ status: "loading" });
    try {
      const value = await callHeartbeat();
      setHb({ status: "ok", value });
    } catch (e) {
      setHb({
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
          {hb.status === "ok" ? `v${hb.value.version}` : "v…"}
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
        <div className="mt-auto pt-6">
          <HeartbeatSidebarSummary
            state={hb}
            onRetry={() => void loadHeartbeat()}
          />
        </div>
      </aside>
      <main className="flex flex-1 flex-col overflow-auto p-8">
        {activeItem.render()}
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

function HeartbeatSidebarSummary({
  state,
  onRetry,
}: {
  state: LoadState;
  onRetry: () => void;
}) {
  if (state.status === "loading") {
    return <p className="text-[11px] text-anvil-300">heartbeat: loading…</p>;
  }
  if (state.status === "error") {
    return (
      <div className="space-y-1">
        <p role="alert" className="text-[11px] text-red-300">
          heartbeat failed
        </p>
        <button
          type="button"
          onClick={onRetry}
          className="rounded border border-anvil-700 bg-anvil-800 px-1.5 py-0.5 text-[10px] text-anvil-50 hover:bg-anvil-700 focus:outline-none focus-visible:ring-2 focus-visible:ring-anvil-300"
        >
          Retry
        </button>
      </div>
    );
  }
  const v = state.value;
  return (
    <div className="space-y-0.5 font-mono text-[10px] text-anvil-300">
      <p>
        os: <span className="text-anvil-100">{v.os.family}</span>
      </p>
      <p>
        adb:{" "}
        <span className="text-anvil-100">{v.adb.path ? "ok" : "missing"}</span>
      </p>
      <p>tauri: v{v.tauri_version}</p>
    </div>
  );
}
