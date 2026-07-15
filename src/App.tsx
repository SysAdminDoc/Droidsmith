import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { ChangeEvent, ReactNode } from "react";
import { useTranslation } from "react-i18next";

import { callHeartbeat, inTauri, type Heartbeat } from "./lib/tauri";
import { startDeviceLifecycle, stopDeviceLifecycle } from "./lib/deviceStore";
import { cn } from "./lib/cn";
import {
  normalizeLanguage,
  persistLanguage,
  SUPPORTED_LANGUAGES,
} from "./lib/i18n";
import { CommandPalette, type PaletteItem } from "./routes/CommandPalette";
import { useCommandPalette } from "./routes/useCommandPalette";
import { useFocusTrap } from "./lib/useFocusTrap";
import OnboardingTour from "./routes/Onboarding";
import DiagnosticsCenter from "./routes/DiagnosticsCenter";

import DevicesRoute from "./routes/Devices";
import WirelessRoute from "./routes/Wireless";
import AppsRoute from "./routes/Apps";
import DebloatRoute from "./routes/Debloat";
import ConsoleRoute from "./routes/Console";
import LogcatRoute from "./routes/Logcat";
import MirrorRoute from "./routes/Mirror";
import FastbootRoute from "./routes/Fastboot";
import { Badge, Button, SkeletonLine } from "./routes/common";

export type NavItem = {
  id:
    | "devices"
    | "wireless"
    | "apps"
    | "debloat"
    | "mirror"
    | "console"
    | "logcat"
    | "fastboot";
  labelKey: string;
  milestone: string;
  descriptionKey: string;
  render: () => ReactNode;
};

/** Single source of truth for the sidebar. Exported so tests can verify
 *  the list stays aligned with `ROADMAP.md` without duplicating it. */
export const NAV_ITEMS: ReadonlyArray<NavItem> = [
  {
    id: "devices",
    labelKey: "nav.devices",
    milestone: "R-012",
    descriptionKey: "devices.description",
    render: () => <DevicesRoute />,
  },
  {
    id: "wireless",
    labelKey: "nav.wireless",
    milestone: "R-015",
    descriptionKey: "wireless.description",
    render: () => <WirelessRoute />,
  },
  {
    id: "apps",
    labelKey: "nav.apps",
    milestone: "R-020",
    descriptionKey: "apps.description",
    render: () => <AppsRoute />,
  },
  {
    id: "debloat",
    labelKey: "nav.debloat",
    milestone: "R-033",
    descriptionKey: "debloat.description",
    render: () => <DebloatRoute />,
  },
  {
    id: "mirror",
    labelKey: "nav.mirror",
    milestone: "R-040",
    descriptionKey: "mirror.description",
    render: () => <MirrorRoute />,
  },
  {
    id: "console",
    labelKey: "nav.console",
    milestone: "R-050",
    descriptionKey: "console.description",
    render: () => <ConsoleRoute />,
  },
  {
    id: "logcat",
    labelKey: "nav.logcat",
    milestone: "R-051",
    descriptionKey: "logcat.description",
    render: () => <LogcatRoute />,
  },
  {
    id: "fastboot",
    labelKey: "nav.fastboot",
    milestone: "R-052",
    descriptionKey: "fastboot.description",
    render: () => <FastbootRoute />,
  },
] as const;

type LoadState =
  | { status: "loading" }
  | { status: "desktop_unavailable" }
  | { status: "ok"; value: Heartbeat }
  | { status: "error"; error: string };

export default function App() {
  const { t } = useTranslation();
  const [hb, setHb] = useState<LoadState>({ status: "loading" });
  const [active, setActive] = useState<NavItem["id"]>(NAV_ITEMS[0].id);
  const [showOnboarding, setShowOnboarding] = useState(false);
  const [showDiagnostics, setShowDiagnostics] = useState(false);
  const [routeAnnouncement, setRouteAnnouncement] = useState<
    NavItem["id"] | null
  >(null);
  const mainRef = useRef<HTMLElement>(null);
  const palette = useCommandPalette();

  const paletteItems: PaletteItem[] = useMemo(
    () =>
      NAV_ITEMS.map((item) => ({
        id: `nav:${item.id}`,
        label: t(item.labelKey),
        description: t(item.descriptionKey),
        category: t("palette.navigationCategory"),
      })),
    [t],
  );

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

  useEffect(() => {
    startDeviceLifecycle();
    return () => {
      void stopDeviceLifecycle();
    };
  }, []);

  const activeItem = NAV_ITEMS.find((i) => i.id === active) ?? NAV_ITEMS[0];
  const activateRoute = useCallback((route: NavItem["id"]) => {
    setActive(route);
    setRouteAnnouncement(route);
    setTimeout(() => mainRef.current?.focus(), 0);
  }, []);

  return (
    <div className="min-h-full overflow-hidden bg-[#08090d] text-anvil-100">
      <a
        href="#main-content"
        className="sr-only focus:not-sr-only focus:fixed focus:left-4 focus:top-4 focus:z-50 focus:rounded-md focus:bg-circuit-300 focus:px-4 focus:py-2 focus:text-sm focus:font-medium focus:text-anvil-950"
      >
        {t("app.skipToContent")}
      </a>
      <div
        className="pointer-events-none fixed inset-0 bg-[linear-gradient(180deg,rgba(255,255,255,0.035),transparent_28rem),linear-gradient(90deg,rgba(34,211,238,0.055),transparent_36rem)]"
        aria-hidden="true"
      />
      <div className="relative flex min-h-full flex-col lg:flex-row">
        <aside
          className="border-b border-white/10 bg-anvil-950/90 p-4 backdrop-blur-xl lg:sticky lg:top-0 lg:flex lg:h-screen lg:w-80 lg:shrink-0 lg:flex-col lg:overflow-y-auto lg:border-b-0 lg:border-r lg:p-5"
          aria-label={t("app.sidebarLabel")}
        >
          <div className="flex items-start justify-between gap-4 lg:block">
            <Brand state={hb} />
            <div className="lg:mt-6">
              <RuntimeBadge state={hb} />
            </div>
          </div>

          <nav
            className="nav-strip mt-5 flex snap-x gap-2 overflow-x-auto pb-2 text-sm lg:block lg:space-y-1.5 lg:overflow-visible lg:pb-0"
            aria-label={t("app.primaryNav")}
          >
            {NAV_ITEMS.map((item) => (
              <NavStub
                key={item.id}
                item={item}
                label={t(item.labelKey)}
                description={t(item.descriptionKey)}
                active={active === item.id}
                onActivate={() => activateRoute(item.id)}
              />
            ))}
          </nav>

          <ShellActions
            className="mt-3 lg:hidden"
            onOpenPalette={() => palette.setOpen(true)}
            onOpenGuide={() => setShowOnboarding(true)}
            onOpenDiagnostics={() => setShowDiagnostics(true)}
          />
          <ShellActions
            className="mt-4 hidden lg:grid"
            onOpenPalette={() => palette.setOpen(true)}
            onOpenGuide={() => setShowOnboarding(true)}
            onOpenDiagnostics={() => setShowDiagnostics(true)}
          />
          <LanguageSelector className="mt-3" />

          <div className="runtime-panel mt-5 hidden lg:block lg:mt-auto lg:pt-8">
            <HeartbeatSidebarSummary
              state={hb}
              onRetry={() => void loadHeartbeat()}
            />
          </div>
        </aside>

        <main
          ref={mainRef}
          id="main-content"
          tabIndex={-1}
          aria-label={t(activeItem.labelKey)}
          className="min-w-0 flex-1 overflow-auto px-4 py-5 sm:px-6 lg:px-8 lg:py-8"
        >
          <div className="mx-auto max-w-7xl">{activeItem.render()}</div>
        </main>
      </div>
      <div
        className="sr-only"
        role="status"
        aria-live="polite"
        aria-atomic="true"
      >
        {routeAnnouncement
          ? t("app.routeChanged", {
              route: t(
                NAV_ITEMS.find((item) => item.id === routeAnnouncement)
                  ?.labelKey ?? NAV_ITEMS[0].labelKey,
              ),
            })
          : ""}
      </div>
      {showOnboarding && (
        <OnboardingModal onDismiss={() => setShowOnboarding(false)} />
      )}
      {showDiagnostics && (
        <DiagnosticsCenter onDismiss={() => setShowDiagnostics(false)} />
      )}
      <CommandPalette
        open={palette.open}
        onClose={() => palette.setOpen(false)}
        items={paletteItems}
        onSelect={(item) => {
          if (item.id.startsWith("nav:")) {
            const navId = item.id.slice(4) as NavItem["id"];
            activateRoute(navId);
          }
        }}
      />
    </div>
  );
}

function OnboardingModal({ onDismiss }: { onDismiss: () => void }) {
  const trapRef = useFocusTrap<HTMLDivElement>();
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") onDismiss();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [onDismiss]);

  return (
    <div
      ref={trapRef}
      role="dialog"
      aria-modal="true"
      aria-labelledby="onboarding-title"
      tabIndex={-1}
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4 outline-none backdrop-blur-sm"
    >
      <OnboardingTour onDismiss={onDismiss} />
    </div>
  );
}

function ShellActions({
  className,
  onOpenPalette,
  onOpenGuide,
  onOpenDiagnostics,
}: {
  className?: string;
  onOpenPalette: () => void;
  onOpenGuide: () => void;
  onOpenDiagnostics: () => void;
}) {
  const { t } = useTranslation();

  return (
    <div className={cn("grid grid-cols-2 gap-2", className)}>
      <Button
        type="button"
        variant="secondary"
        size="sm"
        className="w-full"
        onClick={onOpenPalette}
      >
        <SearchIcon />
        {t("palette.commandsButton")}
      </Button>
      <Button
        type="button"
        variant="ghost"
        size="sm"
        className="w-full"
        onClick={onOpenGuide}
      >
        <GuideIcon />
        {t("app.guide")}
      </Button>
      <Button
        type="button"
        variant="ghost"
        size="sm"
        className="col-span-2 w-full"
        onClick={onOpenDiagnostics}
      >
        <DiagnosticsIcon />
        {t("diagnostics.open")}
      </Button>
    </div>
  );
}

function LanguageSelector({ className }: { className?: string }) {
  const { t, i18n } = useTranslation();
  const currentLanguage =
    normalizeLanguage(i18n.resolvedLanguage ?? i18n.language) ?? "en";

  const handleChange = useCallback(
    (event: ChangeEvent<HTMLSelectElement>) => {
      const nextLanguage = normalizeLanguage(event.target.value);
      if (!nextLanguage) return;
      persistLanguage(nextLanguage);
      void i18n.changeLanguage(nextLanguage);
    },
    [i18n],
  );

  return (
    <label className={cn("block", className)}>
      <span className="mb-1.5 block text-[11px] font-semibold uppercase tracking-[0.08em] text-anvil-500">
        {t("language.label")}
      </span>
      <select
        value={currentLanguage}
        onChange={handleChange}
        className="h-9 w-full rounded-md border border-white/10 bg-white/[0.06] px-3 text-sm text-anvil-50 outline-none transition hover:border-white/20 focus:border-circuit-300/60 focus:ring-2 focus:ring-circuit-300/20"
      >
        {SUPPORTED_LANGUAGES.map((language) => (
          <option key={language.code} value={language.code}>
            {t(language.labelKey)}
          </option>
        ))}
      </select>
    </label>
  );
}

function Brand({ state }: { state: LoadState }) {
  const { t } = useTranslation();

  return (
    <div className="min-w-0">
      <div className="flex items-center gap-3">
        <LogoMark />
        <div className="min-w-0">
          <h1 className="truncate text-lg font-semibold tracking-tight text-anvil-50">
            {t("app.name")}
          </h1>
          <p className="mt-0.5 text-xs leading-5 text-anvil-400">
            {t("app.tagline")}
          </p>
        </div>
      </div>
      <p className="mt-4 hidden max-w-[17rem] text-xs leading-5 text-anvil-400 lg:block">
        {t("app.description")}
      </p>
      {state.status === "ok" && (
        <p className="mt-3 text-xs text-anvil-400">
          {t("app.version", { version: state.value.version })}
        </p>
      )}
    </div>
  );
}

function RuntimeBadge({ state }: { state: LoadState }) {
  const { t } = useTranslation();

  if (state.status === "ok") {
    return <Badge tone="success">{t("runtime.desktop")}</Badge>;
  }
  if (state.status === "loading") {
    return <Badge tone="info">{t("runtime.starting")}</Badge>;
  }
  if (state.status === "error") {
    return <Badge tone="danger">{t("runtime.issue")}</Badge>;
  }
  return <Badge tone="neutral">{t("runtime.browserPreview")}</Badge>;
}

function NavStub({
  item,
  label,
  description,
  active,
  onActivate,
}: {
  item: NavItem;
  label: string;
  description: string;
  active: boolean;
  onActivate: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onActivate}
      aria-current={active ? "page" : undefined}
      aria-describedby={`${item.id}-description`}
      className={cn(
        "group flex min-w-[12.5rem] items-start gap-3 rounded-lg border p-3 text-left transition duration-150 lg:min-w-0",
        "snap-start",
        "focus:outline-none focus-visible:ring-2 focus-visible:ring-circuit-300 focus-visible:ring-offset-2 focus-visible:ring-offset-anvil-950",
        active
          ? "border-circuit-300/30 bg-circuit-300/10 text-anvil-50 shadow-[inset_0_1px_0_rgba(255,255,255,0.08)]"
          : "border-transparent text-anvil-300 hover:border-white/10 hover:bg-white/[0.05] hover:text-anvil-50",
      )}
    >
      <NavIcon id={item.id} active={active} />
      <span className="min-w-0 flex-1">
        <span className="flex items-center justify-between gap-3">
          <span className="font-medium">{label}</span>
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
          id={`${item.id}-description`}
          className={cn(
            "mt-1 text-xs leading-5 text-anvil-400",
            active ? "hidden lg:block" : "sr-only",
          )}
        >
          {description}
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
  const { t } = useTranslation();

  if (state.status === "loading") {
    return (
      <div className="rounded-lg border border-white/10 bg-white/[0.04] p-4">
        <p className="text-xs font-medium text-anvil-200">
          {t("runtime.checking")}
        </p>
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
            {t("runtime.notAttached")}
          </p>
        </div>
        <p className="mt-2 text-xs leading-5 text-anvil-400">
          {t("runtime.notAttachedPrefix")} <code>npm run tauri:dev</code>.
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
            {t("runtime.failed")}
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
          {t("runtime.retry")}
        </Button>
      </div>
    );
  }

  const v = state.value;
  return (
    <div className="rounded-lg border border-white/10 bg-white/[0.04] p-4">
      <div className="flex items-center gap-2">
        <StatusDot tone="success" />
        <p className="text-xs font-medium text-anvil-200">
          {t("runtime.healthy")}
        </p>
      </div>
      <dl className="mt-3 grid gap-2 text-xs">
        <Metric label={t("runtime.os")} value={`${v.os.family} ${v.os.arch}`} />
        <Metric
          label={t("runtime.adb")}
          value={v.adb.path ? t("runtime.resolved") : t("runtime.missing")}
        />
        <Metric
          label={t("runtime.adbStatus")}
          value={t(`runtime.adbStatusValue.${v.adb.compatibility.status}`)}
        />
        <Metric
          label={t("runtime.adbVersion")}
          value={v.adb.version ?? t("common.notReported")}
        />
        <Metric
          label={t("runtime.adbPath")}
          value={v.adb.path ?? t("common.notReported")}
        />
        <Metric label="Tauri" value={`v${v.tauri_version}`} />
      </dl>
    </div>
  );
}

function Metric({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex items-center justify-between gap-3">
      <dt className="text-anvil-500">{label}</dt>
      <dd
        className="truncate text-right font-mono text-anvil-200"
        title={value}
      >
        {value}
      </dd>
    </div>
  );
}

function StatusDot({ tone }: { tone: "neutral" | "success" | "danger" }) {
  return (
    <span
      className={cn(
        "h-2 w-2 rounded-sm",
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

function SearchIcon() {
  return (
    <svg
      viewBox="0 0 24 24"
      className="h-4 w-4"
      fill="none"
      stroke="currentColor"
      strokeLinecap="round"
      strokeLinejoin="round"
      strokeWidth="1.8"
      aria-hidden="true"
    >
      <circle cx="11" cy="11" r="7" />
      <path d="m16.2 16.2 3.3 3.3" />
    </svg>
  );
}

function GuideIcon() {
  return (
    <svg
      viewBox="0 0 24 24"
      className="h-4 w-4"
      fill="none"
      stroke="currentColor"
      strokeLinecap="round"
      strokeLinejoin="round"
      strokeWidth="1.8"
      aria-hidden="true"
    >
      <path d="M5 5.5A2.5 2.5 0 0 1 7.5 3H19v16H7.5A2.5 2.5 0 0 0 5 21.5v-16Z" />
      <path d="M9 7h6M9 10h5" />
    </svg>
  );
}

function DiagnosticsIcon() {
  return (
    <svg
      viewBox="0 0 24 24"
      className="h-4 w-4"
      fill="none"
      stroke="currentColor"
      strokeLinecap="round"
      strokeLinejoin="round"
      strokeWidth="1.8"
      aria-hidden="true"
    >
      <path d="M5 5.5h14v13H5zM8 9h3M8 12h8M8 15h6" />
      <path d="m15.5 7.5 1 1 2-2" />
    </svg>
  );
}

function NavIcon({ id, active }: { id: NavItem["id"]; active: boolean }) {
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
      {id === "devices" && (
        <>
          <path d="M8 4.5h8a2 2 0 0 1 2 2v11a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2v-11a2 2 0 0 1 2-2Z" />
          <path d="M10 7.5h4M10 16.5h4" />
        </>
      )}
      {id === "wireless" && (
        <>
          <path d="M7.25 14.25a6.75 6.75 0 0 1 9.5 0M4.5 11.25a10.6 10.6 0 0 1 15 0M10 17.25a2.85 2.85 0 0 1 4 0" />
          <path d="M12 20h.01" />
        </>
      )}
      {id === "apps" && (
        <>
          <path d="M5 5h5v5H5zM14 5h5v5h-5zM5 14h5v5H5zM14 14h5v5h-5z" />
        </>
      )}
      {id === "debloat" && (
        <>
          <path d="M12 4.5 18 7v4.5c0 3.75-2.4 6.6-6 8-3.6-1.4-6-4.25-6-8V7l6-2.5Z" />
          <path d="m9.5 12.25 1.7 1.7 3.6-4" />
        </>
      )}
      {id === "mirror" && (
        <>
          <path d="M8 4.5h8a2 2 0 0 1 2 2v11a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2v-11a2 2 0 0 1 2-2Z" />
          <path d="M9 8h6v5H9zM10.5 16.5h3" />
        </>
      )}
      {id === "console" && (
        <>
          <path d="m5 7 4 4-4 4M11 16h8" />
        </>
      )}
      {id === "logcat" && (
        <>
          <path d="M6 6h12M6 10h8M6 14h12M6 18h6" />
        </>
      )}
      {id === "fastboot" && (
        <>
          <path d="m13 3.5-7 10h5l-1 7 7-10h-5l1-7Z" />
        </>
      )}
    </svg>
  );
}
