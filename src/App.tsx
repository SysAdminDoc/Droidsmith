import { useCallback, useEffect, useState } from "react";
import type { ReactNode } from "react";

import { callHeartbeat, inTauri, type Heartbeat } from "./lib/tauri";
import { cn } from "./lib/cn";

import DevicesRoute from "./routes/Devices";
import WirelessRoute from "./routes/Wireless";
import AppsRoute from "./routes/Apps";
import DebloatRoute from "./routes/Debloat";
import { Badge, Button, SkeletonLine } from "./routes/common";
import {
  ConsoleRoute,
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
    description: "USB + wireless discovery for every connected Android target.",
    render: () => <DevicesRoute />,
  },
  {
    label: "Wireless",
    milestone: "R-015",
    description:
      "QR, pairing-code, and mDNS flows for Android wireless debugging.",
    render: () => <WirelessRoute />,
  },
  {
    label: "Apps",
    milestone: "R-020",
    description: "Installed packages, filters, icons, and bulk actions.",
    render: () => <AppsRoute />,
  },
  {
    label: "Debloat",
    milestone: "R-033",
    description: "Pack preview, vendor warnings, journaled apply and undo.",
    render: () => <DebloatRoute />,
  },
  {
    label: "Mirror",
    milestone: "R-040",
    description: "scrcpy sessions with audio, recording, and drag installs.",
    render: () => <MirrorRoute />,
  },
  {
    label: "Console",
    milestone: "R-050",
    description: "Multi-tab adb shell with history and reusable snippets.",
    render: () => <ConsoleRoute />,
  },
  {
    label: "Logcat",
    milestone: "R-051",
    description: "Live logs with tag, pid, level, and grep filters.",
    render: () => <LogcatRoute />,
  },
  {
    label: "Fastboot",
    milestone: "R-052",
    description: "Bootloader devices, partitions, slots, and safety checks.",
    render: () => <FastbootRoute />,
  },
] as const;

type LoadState =
  | { status: "loading" }
  | { status: "desktop_unavailable" }
  | { status: "ok"; value: Heartbeat }
  | { status: "error"; error: string };

export default function App() {
  const [hb, setHb] = useState<LoadState>({ status: "loading" });
  const [active, setActive] = useState<string>(NAV_ITEMS[0].label);

  const loadHeartbeat = useCallback(async () => {
    if (!inTauri()) {
      setHb({ status: "desktop_unavailable" });
      return;
    }

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
    <div className="min-h-full overflow-hidden bg-[#08090d] text-anvil-100">
      <div
        className="pointer-events-none fixed inset-0 bg-[radial-gradient(circle_at_top_left,rgba(34,211,238,0.12),transparent_34rem),linear-gradient(135deg,rgba(255,255,255,0.04),transparent_34%)]"
        aria-hidden="true"
      />
      <div className="relative flex min-h-full flex-col lg:flex-row">
        <aside className="border-b border-white/10 bg-anvil-950/90 p-4 backdrop-blur-xl lg:sticky lg:top-0 lg:flex lg:h-screen lg:w-80 lg:shrink-0 lg:flex-col lg:overflow-y-auto lg:border-b-0 lg:border-r lg:p-5">
          <div className="flex items-start justify-between gap-4 lg:block">
            <Brand state={hb} />
            <div className="lg:mt-6">
              <RuntimeBadge state={hb} />
            </div>
          </div>

          <nav
            className="nav-strip mt-5 flex snap-x gap-2 overflow-x-auto pb-1 text-sm lg:block lg:space-y-1.5 lg:overflow-visible lg:pb-0"
            aria-label="Primary"
          >
            {NAV_ITEMS.map((item) => (
              <NavStub
                key={item.label}
                item={item}
                active={active === item.label}
                onActivate={() => setActive(item.label)}
              />
            ))}
          </nav>

          <div className="runtime-panel mt-5 hidden lg:block lg:mt-auto lg:pt-8">
            <HeartbeatSidebarSummary
              state={hb}
              onRetry={() => void loadHeartbeat()}
            />
          </div>
        </aside>

        <main className="min-w-0 flex-1 overflow-auto px-4 py-5 sm:px-6 lg:px-8 lg:py-8">
          <div className="mx-auto max-w-7xl">{activeItem.render()}</div>
        </main>
      </div>
    </div>
  );
}

function Brand({ state }: { state: LoadState }) {
  return (
    <div className="min-w-0">
      <div className="flex items-center gap-3">
        <LogoMark />
        <div className="min-w-0">
          <h1 className="truncate text-lg font-semibold tracking-tight text-anvil-50">
            Droidsmith
          </h1>
          <p className="mt-0.5 text-xs leading-5 text-anvil-400">
            Android device workshop
          </p>
        </div>
      </div>
      <p className="mt-4 hidden max-w-[17rem] text-xs leading-5 text-anvil-400 lg:block">
        Inspect devices, stage actions, and keep every risky Android operation
        traceable.
      </p>
      {state.status === "ok" && (
        <p className="mt-3 text-xs text-anvil-400">
          Version {state.value.version}
        </p>
      )}
    </div>
  );
}

function RuntimeBadge({ state }: { state: LoadState }) {
  if (state.status === "ok") {
    return <Badge tone="success">Desktop runtime</Badge>;
  }
  if (state.status === "loading") {
    return <Badge tone="info">Starting</Badge>;
  }
  if (state.status === "error") {
    return <Badge tone="danger">Runtime issue</Badge>;
  }
  return <Badge tone="neutral">Browser preview</Badge>;
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
      aria-describedby={`${item.label}-description`}
      className={cn(
        "group flex min-w-[12.5rem] items-start gap-3 rounded-lg border p-3 text-left transition duration-150 lg:min-w-0",
        "snap-start",
        "focus:outline-none focus-visible:ring-2 focus-visible:ring-circuit-300 focus-visible:ring-offset-2 focus-visible:ring-offset-anvil-950",
        active
          ? "border-circuit-300/30 bg-circuit-300/10 text-anvil-50 shadow-[inset_0_1px_0_rgba(255,255,255,0.08)]"
          : "border-transparent text-anvil-300 hover:border-white/10 hover:bg-white/[0.05] hover:text-anvil-50",
      )}
    >
      <NavIcon label={item.label} active={active} />
      <span className="min-w-0 flex-1">
        <span className="flex items-center justify-between gap-3">
          <span className="font-medium">{item.label}</span>
          <span
            className={cn(
              "font-mono text-[10px] uppercase tracking-[0.08em]",
              active ? "text-circuit-100" : "text-anvil-500",
            )}
          >
            {item.milestone}
          </span>
        </span>
        <span
          id={`${item.label}-description`}
          className={cn(
            "mt-1 text-xs leading-5 text-anvil-400",
            active ? "hidden lg:block" : "sr-only",
          )}
        >
          {item.description}
        </span>
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
    return (
      <div className="rounded-lg border border-white/10 bg-white/[0.04] p-4">
        <p className="text-xs font-medium text-anvil-200">Checking runtime</p>
        <div className="mt-3 space-y-2" aria-hidden="true">
          <SkeletonLine className="w-4/5" />
          <SkeletonLine className="w-2/3" />
          <SkeletonLine className="w-3/4" />
        </div>
      </div>
    );
  }

  if (state.status === "desktop_unavailable") {
    return (
      <div className="rounded-lg border border-white/10 bg-white/[0.04] p-4">
        <div className="flex items-center gap-2">
          <StatusDot tone="neutral" />
          <p className="text-xs font-medium text-anvil-200">
            Desktop shell not attached
          </p>
        </div>
        <p className="mt-2 text-xs leading-5 text-anvil-400">
          Device IPC is available when the app is launched with{" "}
          <code>npm run tauri:dev</code>.
        </p>
      </div>
    );
  }

  if (state.status === "error") {
    return (
      <div className="rounded-lg border border-red-300/20 bg-red-950/20 p-4">
        <div className="flex items-center gap-2">
          <StatusDot tone="danger" />
          <p role="alert" className="text-xs font-medium text-red-100">
            Runtime check failed
          </p>
        </div>
        <p className="mt-2 line-clamp-3 text-xs leading-5 text-red-100/75">
          {state.error}
        </p>
        <Button
          type="button"
          onClick={onRetry}
          variant="danger"
          size="sm"
          className="mt-3"
        >
          Retry
        </Button>
      </div>
    );
  }

  const v = state.value;
  return (
    <div className="rounded-lg border border-white/10 bg-white/[0.04] p-4">
      <div className="flex items-center gap-2">
        <StatusDot tone="success" />
        <p className="text-xs font-medium text-anvil-200">Runtime healthy</p>
      </div>
      <dl className="mt-3 grid gap-2 text-xs">
        <Metric label="OS" value={`${v.os.family} ${v.os.arch}`} />
        <Metric label="ADB" value={v.adb.path ? "Resolved" : "Missing"} />
        <Metric label="Tauri" value={`v${v.tauri_version}`} />
      </dl>
    </div>
  );
}

function Metric({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex items-center justify-between gap-3">
      <dt className="text-anvil-500">{label}</dt>
      <dd className="truncate text-right font-mono text-anvil-200">{value}</dd>
    </div>
  );
}

function StatusDot({ tone }: { tone: "neutral" | "success" | "danger" }) {
  return (
    <span
      className={cn(
        "h-2 w-2 rounded-full",
        tone === "neutral" && "bg-anvil-400",
        tone === "success" && "bg-emerald-300",
        tone === "danger" && "bg-red-300",
      )}
      aria-hidden="true"
    />
  );
}

function LogoMark() {
  return (
    <span
      className="grid h-10 w-10 shrink-0 place-items-center rounded-lg border border-circuit-300/20 bg-circuit-300/10 text-circuit-100 shadow-[inset_0_1px_0_rgba(255,255,255,0.08)]"
      aria-hidden="true"
    >
      <svg viewBox="0 0 24 24" className="h-5 w-5" fill="none">
        <path
          d="M7 8.25h10M8.5 5.5h7M8 8.25v7.25a3 3 0 0 0 3 3h2a3 3 0 0 0 3-3V8.25M9 14h6M10 11.25h.01M14 11.25h.01"
          stroke="currentColor"
          strokeLinecap="round"
          strokeLinejoin="round"
          strokeWidth="1.7"
        />
      </svg>
    </span>
  );
}

function NavIcon({ label, active }: { label: string; active: boolean }) {
  const className = cn(
    "mt-0.5 h-5 w-5 shrink-0 transition",
    active ? "text-circuit-100" : "text-anvil-500 group-hover:text-anvil-300",
  );

  return (
    <svg
      viewBox="0 0 24 24"
      className={className}
      fill="none"
      stroke="currentColor"
      strokeLinecap="round"
      strokeLinejoin="round"
      strokeWidth="1.75"
      aria-hidden="true"
    >
      {label === "Devices" && (
        <>
          <path d="M8 4.5h8a2 2 0 0 1 2 2v11a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2v-11a2 2 0 0 1 2-2Z" />
          <path d="M10 7.5h4M10 16.5h4" />
        </>
      )}
      {label === "Wireless" && (
        <>
          <path d="M7.25 14.25a6.75 6.75 0 0 1 9.5 0M4.5 11.25a10.6 10.6 0 0 1 15 0M10 17.25a2.85 2.85 0 0 1 4 0" />
          <path d="M12 20h.01" />
        </>
      )}
      {label === "Apps" && (
        <>
          <path d="M5 5h5v5H5zM14 5h5v5h-5zM5 14h5v5H5zM14 14h5v5h-5z" />
        </>
      )}
      {label === "Debloat" && (
        <>
          <path d="M12 4.5 18 7v4.5c0 3.75-2.4 6.6-6 8-3.6-1.4-6-4.25-6-8V7l6-2.5Z" />
          <path d="m9.5 12.25 1.7 1.7 3.6-4" />
        </>
      )}
      {label === "Mirror" && (
        <>
          <path d="M8 4.5h8a2 2 0 0 1 2 2v11a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2v-11a2 2 0 0 1 2-2Z" />
          <path d="M9 8h6v5H9zM10.5 16.5h3" />
        </>
      )}
      {label === "Console" && (
        <>
          <path d="m5 7 4 4-4 4M11 16h8" />
        </>
      )}
      {label === "Logcat" && (
        <>
          <path d="M6 6h12M6 10h8M6 14h12M6 18h6" />
        </>
      )}
      {label === "Fastboot" && (
        <>
          <path d="m13 3.5-7 10h5l-1 7 7-10h-5l1-7Z" />
        </>
      )}
    </svg>
  );
}
