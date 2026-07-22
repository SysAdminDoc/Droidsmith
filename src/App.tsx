import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { ChangeEvent, ReactNode } from "react";
import { useTranslation } from "react-i18next";

import {
  callHeartbeat,
  errorMessage,
  inTauri,
  type Heartbeat,
} from "./lib/tauri";
import { startDeviceLifecycle, stopDeviceLifecycle } from "./lib/deviceStore";
import { cn } from "./lib/cn";
import { normalizeLanguage, SUPPORTED_LANGUAGES } from "./lib/i18n";
import { setStoredLanguage } from "./lib/settings";
import { CommandPalette, type PaletteItem } from "./routes/CommandPalette";
import { useCommandPalette } from "./routes/useCommandPalette";
import { useFocusTrap } from "./lib/useFocusTrap";
import OnboardingTour from "./routes/Onboarding";
import DiagnosticsCenter from "./routes/DiagnosticsCenter";
import SettingsDataControls from "./routes/SettingsDataControls";

import DevicesRoute from "./routes/Devices";
import WirelessRoute from "./routes/Wireless";
import AppsRoute from "./routes/Apps";
import DebloatRoute from "./routes/Debloat";
import ApkAnalyzerRoute from "./routes/ApkAnalyzer";
import ProfilesRoute from "./routes/Profiles";
import ConsoleRoute from "./routes/Console";
import LogcatRoute from "./routes/Logcat";
import MirrorRoute from "./routes/Mirror";
import FastbootRoute from "./routes/Fastboot";
import DeviceSettingsRoute from "./routes/DeviceSettings";
import { Button } from "./routes/common";
import droidsmithLogo from "./assets/droidsmith-logo.png";

export type NavItem = {
  id:
    | "devices"
    | "wireless"
    | "apps"
    | "debloat"
    | "profiles"
    | "mirror"
    | "console"
    | "logcat"
    | "fastboot"
    | "apk-analyzer"
    | "tuning";
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
    id: "profiles",
    labelKey: "nav.profiles",
    milestone: "R-034",
    descriptionKey: "profiles.description",
    render: () => <ProfilesRoute />,
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
  {
    id: "tuning",
    labelKey: "nav.tuning",
    milestone: "R-082",
    descriptionKey: "tuning.description",
    render: () => <DeviceSettingsRoute />,
  },
  {
    id: "apk-analyzer",
    labelKey: "nav.apkAnalyzer",
    milestone: "R-097",
    descriptionKey: "apk.description",
    render: () => <ApkAnalyzerRoute />,
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
  const [showSettings, setShowSettings] = useState(false);
  const [showAbout, setShowAbout] = useState(false);
  const [routeAnnouncement, setRouteAnnouncement] = useState<
    NavItem["id"] | null
  >(null);
  const mainRef = useRef<HTMLElement>(null);
  const sidebarRef = useRef<HTMLElement>(null);
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
    setTimeout(() => {
      window.scrollTo({ top: 0, left: 0 });
      sidebarRef.current?.scrollTo({ top: 0, left: 0 });
      mainRef.current?.scrollTo({ top: 0, left: 0 });
      mainRef.current?.focus();
    }, 0);
  }, []);

  return (
    <div className="min-h-full overflow-hidden bg-anvil-950 text-anvil-100 lg:h-full">
      <a
        href="#main-content"
        className="sr-only focus:not-sr-only focus:fixed focus:start-4 focus:top-4 focus:z-50 focus:rounded-md focus:bg-circuit-300 focus:px-4 focus:py-2 focus:text-sm focus:font-medium focus:text-anvil-950"
      >
        {t("app.skipToContent")}
      </a>
      <div className="relative flex min-h-full flex-col lg:h-full lg:flex-row">
        <aside
          ref={sidebarRef}
          className="border-b border-white/[0.08] bg-[#11151b] p-4 lg:sticky lg:top-0 lg:flex lg:h-screen lg:w-[15.5rem] lg:shrink-0 lg:flex-col lg:overflow-y-auto lg:border-b-0 lg:border-e lg:p-4"
          aria-label={t("app.sidebarLabel")}
        >
          <div className="flex items-start justify-between gap-4 lg:block">
            <Brand state={hb} />
          </div>

          <nav
            className="nav-strip mt-4 flex snap-x gap-2 overflow-x-auto pb-2 text-sm lg:block lg:space-y-1 lg:overflow-visible lg:pb-0"
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

          <div className="mt-3 lg:mt-auto lg:border-t lg:border-white/[0.08] lg:pt-3">
            <ShellActions
              onOpenSettings={() => setShowSettings(true)}
              onOpenGuide={() => setShowOnboarding(true)}
              onOpenAbout={() => setShowAbout(true)}
            />
          </div>
        </aside>

        <main
          ref={mainRef}
          id="main-content"
          tabIndex={-1}
          aria-label={t(activeItem.labelKey)}
          className="min-w-0 flex-1 overflow-auto px-4 py-5 sm:px-6 lg:h-full lg:px-8 lg:py-6 xl:px-10"
        >
          <div className="mx-auto max-w-[88rem]">{activeItem.render()}</div>
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
      {showSettings && (
        <SettingsModal onDismiss={() => setShowSettings(false)} />
      )}
      {showAbout && (
        <AboutModal
          state={hb}
          onRetry={() => void loadHeartbeat()}
          onOpenDiagnostics={() => {
            setShowAbout(false);
            setShowDiagnostics(true);
          }}
          onDismiss={() => setShowAbout(false)}
        />
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

function SettingsModal({ onDismiss }: { onDismiss: () => void }) {
  const { t } = useTranslation();
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
      aria-labelledby="settings-modal-title"
      tabIndex={-1}
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4 outline-none backdrop-blur-sm"
    >
      <div className="max-h-[90vh] w-full max-w-xl overflow-y-auto rounded-xl border border-white/[0.09] bg-[#151a21] p-5 shadow-2xl sm:p-6">
        <div className="flex items-start justify-between gap-4 border-b border-white/[0.08] pb-4">
          <div>
            <h2
              id="settings-modal-title"
              className="text-lg font-semibold text-anvil-50"
            >
              {t("app.settings")}
            </h2>
            <p className="mt-1 text-sm text-anvil-400">
              {t("app.settingsDescription")}
            </p>
          </div>
          <Button type="button" variant="ghost" size="sm" onClick={onDismiss}>
            {t("common.close")}
          </Button>
        </div>
        <div className="border-b border-white/[0.08] py-5">
          <p className="text-xs font-medium text-anvil-300">
            {t("language.label")}
          </p>
          <LanguageSelector className="mt-2 max-w-sm" />
        </div>
        <div className="pt-5">
          <SettingsDataControls embedded />
        </div>
      </div>
    </div>
  );
}

function AboutModal({
  state,
  onRetry,
  onOpenDiagnostics,
  onDismiss,
}: {
  state: LoadState;
  onRetry: () => void;
  onOpenDiagnostics: () => void;
  onDismiss: () => void;
}) {
  const { t } = useTranslation();
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
      aria-labelledby="about-modal-title"
      tabIndex={-1}
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4 outline-none backdrop-blur-sm"
    >
      <div className="w-full max-w-md rounded-xl border border-white/[0.09] bg-[#151a21] p-6 shadow-2xl">
        <div className="flex items-start gap-4">
          <LogoMark />
          <div className="min-w-0">
            <h2
              id="about-modal-title"
              className="text-xl font-semibold text-anvil-50"
            >
              {t("app.name")}
            </h2>
            <p className="mt-1 text-sm text-anvil-400">{t("app.tagline")}</p>
          </div>
        </div>
        <p className="mt-5 text-sm leading-6 text-anvil-300">
          {t("app.aboutDescription")}
        </p>
        <AboutRuntime state={state} onRetry={onRetry} />
        <div className="mt-6 flex flex-wrap justify-end gap-2 border-t border-white/[0.08] pt-4">
          <Button type="button" variant="ghost" onClick={onDismiss}>
            {t("common.close")}
          </Button>
          <Button type="button" onClick={onOpenDiagnostics}>
            <DiagnosticsIcon />
            {t("diagnostics.open")}
          </Button>
        </div>
      </div>
    </div>
  );
}

function AboutRuntime({
  state,
  onRetry,
}: {
  state: LoadState;
  onRetry: () => void;
}) {
  const { t } = useTranslation();

  if (state.status === "loading") {
    return (
      <p className="mt-4 text-xs text-anvil-500">{t("runtime.checking")}</p>
    );
  }

  if (state.status === "error") {
    return (
      <div className="mt-4 flex items-center justify-between gap-3 text-xs text-red-200">
        <span>{t("runtime.failed")}</span>
        <Button type="button" variant="ghost" size="sm" onClick={onRetry}>
          {t("runtime.retry")}
        </Button>
      </div>
    );
  }

  return (
    <div className="mt-4 flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-anvil-500">
      <span>
        {state.status === "ok"
          ? t("app.version", { version: state.value.version })
          : t("runtime.browserPreview")}
      </span>
      <span aria-hidden="true">•</span>
      <span>
        {state.status === "ok"
          ? t("runtime.desktop")
          : t("runtime.notAttached")}
      </span>
    </div>
  );
}

function ShellActions({
  className,
  onOpenSettings,
  onOpenGuide,
  onOpenAbout,
}: {
  className?: string;
  onOpenSettings: () => void;
  onOpenGuide: () => void;
  onOpenAbout: () => void;
}) {
  const { t } = useTranslation();

  return (
    <div className={cn("grid grid-cols-3 gap-1", className)}>
      <Button
        type="button"
        variant="ghost"
        size="sm"
        className="w-full px-1.5"
        onClick={onOpenSettings}
      >
        <SettingsIcon />
        {t("app.settings")}
      </Button>
      <Button
        type="button"
        variant="ghost"
        size="sm"
        className="w-full px-1.5"
        onClick={onOpenGuide}
      >
        <HelpIcon />
        {t("app.help")}
      </Button>
      <Button
        type="button"
        variant="ghost"
        size="sm"
        className="w-full px-1.5"
        onClick={onOpenAbout}
      >
        <AboutIcon />
        {t("app.about")}
      </Button>
    </div>
  );
}

function LanguageSelector({ className }: { className?: string }) {
  const { t, i18n } = useTranslation();
  const [persistenceError, setPersistenceError] = useState<string | null>(null);
  const persistenceRevision = useRef(0);
  const persistenceQueue = useRef<Promise<void>>(Promise.resolve());
  const currentLanguage =
    normalizeLanguage(i18n.resolvedLanguage ?? i18n.language) ?? "en";

  const handleChange = useCallback(
    async (event: ChangeEvent<HTMLSelectElement>) => {
      const nextLanguage = normalizeLanguage(event.target.value);
      if (!nextLanguage) return;
      const revision = ++persistenceRevision.current;
      setPersistenceError(null);
      await i18n.changeLanguage(nextLanguage);

      const save = persistenceQueue.current.then(async () => {
        await setStoredLanguage(nextLanguage);
      });
      persistenceQueue.current = save.catch(() => undefined);
      try {
        await save;
      } catch (error) {
        if (revision === persistenceRevision.current) {
          setPersistenceError(errorMessage(error));
        }
      }
    },
    [i18n],
  );

  return (
    <label className={cn("block", className)}>
      <span className="sr-only">{t("language.label")}</span>
      <select
        value={currentLanguage}
        onChange={(event) => void handleChange(event)}
        aria-label={t("language.label")}
        aria-describedby={
          persistenceError ? "language-persistence-error" : undefined
        }
        className="h-9 w-full rounded-md border border-transparent bg-white/[0.035] px-3 text-xs text-anvil-300 outline-none transition hover:bg-white/[0.06] focus:border-circuit-300/60 focus:ring-2 focus:ring-circuit-300/20"
      >
        {SUPPORTED_LANGUAGES.map((language) => (
          <option key={language.code} value={language.code}>
            {t(language.labelKey)}
          </option>
        ))}
      </select>
      {persistenceError && (
        <span
          id="language-persistence-error"
          role="alert"
          className="mt-1 block text-xs text-red-300"
        >
          {t("language.saveFailed", { message: persistenceError })}
        </span>
      )}
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
      {state.status === "ok" && (
        <span className="sr-only">
          {t("app.version", { version: state.value.version })}
        </span>
      )}
    </div>
  );
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
        "group relative flex min-w-[11.5rem] items-center gap-3 rounded-md px-3 py-2.5 text-start transition duration-150 lg:min-w-0",
        "snap-start",
        "focus:outline-none focus-visible:ring-2 focus-visible:ring-circuit-300 focus-visible:ring-offset-2 focus-visible:ring-offset-anvil-950",
        active
          ? "bg-circuit-300/[0.11] text-anvil-50 before:absolute before:inset-y-2 before:start-0 before:w-0.5 before:rounded-full before:bg-circuit-300"
          : "text-anvil-300 hover:bg-white/[0.045] hover:text-anvil-50",
      )}
    >
      <NavIcon id={item.id} active={active} />
      <span className="min-w-0 flex-1">
        <span className="font-medium">{label}</span>
        <span className="sr-only">{item.milestone}</span>
        <span id={`${item.id}-description`} className="sr-only">
          {description}
        </span>
      </span>
    </button>
  );
}

function LogoMark() {
  return (
    <img
      src={droidsmithLogo}
      alt=""
      className="h-14 w-14 shrink-0 rounded-2xl"
      aria-hidden="true"
    />
  );
}

function SettingsIcon() {
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
      <circle cx="12" cy="12" r="3" />
      <path d="M19.4 15a1.7 1.7 0 0 0 .3 1.9l.1.1-2.8 2.8-.1-.1a1.7 1.7 0 0 0-1.9-.3 1.7 1.7 0 0 0-1 1.6v.2h-4V21a1.7 1.7 0 0 0-1-1.6 1.7 1.7 0 0 0-1.9.3l-.1.1L4.2 17l.1-.1a1.7 1.7 0 0 0 .3-1.9A1.7 1.7 0 0 0 3 14H2.8v-4H3a1.7 1.7 0 0 0 1.6-1 1.7 1.7 0 0 0-.3-1.9L4.2 7 7 4.2l.1.1a1.7 1.7 0 0 0 1.9.3A1.7 1.7 0 0 0 10 3v-.2h4V3a1.7 1.7 0 0 0 1 1.6 1.7 1.7 0 0 0 1.9-.3l.1-.1L19.8 7l-.1.1a1.7 1.7 0 0 0-.3 1.9 1.7 1.7 0 0 0 1.6 1h.2v4H21a1.7 1.7 0 0 0-1.6 1Z" />
    </svg>
  );
}

function HelpIcon() {
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
      <circle cx="12" cy="12" r="9" />
      <path d="M9.8 9a2.3 2.3 0 1 1 3.3 2.1c-.8.4-1.1.9-1.1 1.7M12 16.5h.01" />
    </svg>
  );
}

function AboutIcon() {
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
      <circle cx="12" cy="12" r="9" />
      <path d="M12 10.5V17M12 7h.01" />
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
      {id === "profiles" && (
        <>
          <path d="M7 4.5h8l3 3v12H7a2 2 0 0 1-2-2v-11a2 2 0 0 1 2-2Z" />
          <path d="M15 4.5v3h3M8.5 11h6M8.5 14.5h6" />
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
