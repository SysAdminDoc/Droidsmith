import type { ComponentPropsWithoutRef, ReactNode } from "react";
import { useState } from "react";
import { useTranslation } from "react-i18next";

import { cn } from "../lib/cn";
import {
  errorMessage,
  callRevealInFolder,
  requiresTransportOverride,
  type Device,
  type DeviceTarget,
  type DeviceTransportKind,
} from "../lib/tauri";

export function PaneHeader({
  title,
  milestone,
  description,
  actions,
  meta,
}: {
  title: string;
  milestone: string;
  description: string;
  actions?: ReactNode;
  meta?: ReactNode;
}) {
  return (
    <header className="border-b border-white/[0.09] pb-3.5">
      <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
        <div className="min-w-0">
          <div className="flex flex-wrap items-center gap-3">
            <h2 className="text-[1.75rem] font-semibold leading-9 tracking-[-0.025em] text-anvil-50">
              {title}
            </h2>
            <MilestoneBadge milestone={milestone} />
            {meta && <div>{meta}</div>}
          </div>
          <p className="sr-only">{description}</p>
        </div>
        {actions && (
          <div className="flex shrink-0 flex-wrap items-center gap-2 sm:justify-end">
            {actions}
          </div>
        )}
      </div>
    </header>
  );
}

export function Card({
  children,
  className,
  surface = "section",
  ...props
}: ComponentPropsWithoutRef<"div"> & {
  surface?: "section" | "panel" | "dialog";
}) {
  return (
    <div
      className={cn(
        surface === "section" &&
          "rounded-none border-0 border-t border-white/[0.085] bg-transparent shadow-none",
        surface === "panel" &&
          "rounded-sm border border-white/[0.085] bg-black/10 shadow-panel",
        surface === "dialog" &&
          "rounded-lg border border-white/[0.1] bg-[#121820] shadow-2xl",
        className ?? "p-4",
      )}
      {...props}
    >
      {children}
    </div>
  );
}

export function Button({
  children,
  className,
  size = "md",
  variant = "secondary",
  ...props
}: ComponentPropsWithoutRef<"button"> & {
  size?: "sm" | "md";
  variant?: "primary" | "secondary" | "ghost" | "danger";
}) {
  return (
    <button
      type="button"
      className={cn(
        "inline-flex items-center justify-center gap-2 rounded-[0.25rem] font-medium transition duration-150",
        "focus:outline-none focus-visible:ring-2 focus-visible:ring-circuit-300 focus-visible:ring-offset-2 focus-visible:ring-offset-anvil-950",
        "disabled:cursor-not-allowed disabled:opacity-50",
        size === "sm" ? "min-h-9 px-3 text-sm" : "min-h-10 px-4 text-sm",
        variant === "primary" &&
          "bg-circuit-300 text-anvil-950 shadow-sm hover:bg-circuit-200 active:bg-circuit-400",
        variant === "secondary" &&
          "bg-white/[0.045] text-anvil-100 hover:bg-white/[0.08] active:bg-white/[0.11]",
        variant === "ghost" &&
          "text-anvil-200 hover:bg-white/[0.07] hover:text-anvil-50 active:bg-white/[0.1]",
        variant === "danger" &&
          "border border-red-300/20 bg-red-400/10 text-red-100 hover:bg-red-400/15",
        className,
      )}
      {...props}
    >
      {children}
    </button>
  );
}

/// Reveal a Droidsmith-produced artifact in the OS file manager. The backend
/// only honors paths it wrote this session, so passing `path` here can never
/// open an unrelated location; failures (e.g. the file was moved) surface
/// inline rather than throwing.
export function RevealInFolderButton({
  path,
  label,
  size = "sm",
}: {
  path: string;
  label?: string;
  size?: "sm" | "md";
}) {
  const { t } = useTranslation();
  const [error, setError] = useState<string | null>(null);
  return (
    <span className="inline-flex items-center gap-2">
      <Button
        type="button"
        size={size}
        variant="ghost"
        onClick={() => {
          setError(null);
          void callRevealInFolder(path).catch((cause) =>
            setError(errorMessage(cause)),
          );
        }}
      >
        {label ?? t("common.showInFolder")}
      </Button>
      {error && (
        <span role="alert" className="text-xs text-red-300">
          {error}
        </span>
      )}
    </span>
  );
}

/// Shared authorized-device selector shown by every route that targets a single
/// device (Apps, Debloat). Selection prefers the stable transport id and falls
/// back to the serial so a reconnected device keeps its selection.
export function DevicePicker({
  devices,
  selected,
  selectedSerial,
  onSelect,
}: {
  devices: Device[];
  selected: number | null;
  selectedSerial: string | null;
  onSelect: (device: Device) => void;
}) {
  const { t } = useTranslation();

  return (
    <section className="border-b border-white/[0.08] pb-4">
      <h3 className="text-xs font-medium text-anvil-400">
        {t("common.selectDevice")}
      </h3>
      <div className="mt-2 flex flex-wrap gap-2">
        {devices.map((d) => (
          <Button
            key={`${d.transport_id ?? d.serial}:${d.connection_generation}`}
            type="button"
            variant={
              (
                d.transport_id != null
                  ? d.transport_id === selected
                  : d.serial === selectedSerial
              )
                ? "primary"
                : "secondary"
            }
            size="sm"
            onClick={() => onSelect(d)}
          >
            {d.model ?? d.serial}
          </Button>
        ))}
      </div>
    </section>
  );
}

export function Badge({
  children,
  tone = "neutral",
  className,
}: {
  children: ReactNode;
  tone?: "neutral" | "info" | "success" | "warning" | "danger";
  className?: string;
}) {
  return (
    <span
      className={cn(
        "inline-flex min-h-6 items-center gap-1.5 whitespace-nowrap text-xs font-medium",
        tone === "neutral" && "text-anvil-400",
        tone === "info" && "text-circuit-200",
        tone === "success" && "text-emerald-200",
        tone === "warning" && "text-amber-200",
        tone === "danger" && "text-red-200",
        className,
      )}
    >
      {tone !== "neutral" && tone !== "info" && (
        <span
          className={cn(
            "h-1.5 w-1.5 shrink-0 rounded-full",
            tone === "success" && "bg-emerald-300",
            tone === "warning" && "bg-amber-300",
            tone === "danger" && "bg-red-300",
          )}
          aria-hidden="true"
        />
      )}
      {children}
    </span>
  );
}

export function MilestoneBadge({ milestone }: { milestone: string }) {
  const { t } = useTranslation();

  return <span className="sr-only">{t("common.roadmap", { milestone })}</span>;
}

const transportLabels: Record<DeviceTransportKind, string> = {
  usb: "devices.transportUsb",
  tls_wifi: "devices.transportTlsWifi",
  legacy_tcp: "devices.transportLegacyTcp",
  unknown_tcp: "devices.transportUnknownTcp",
};

export function TransportBadge({ kind }: { kind: DeviceTransportKind }) {
  const { t } = useTranslation();
  const tone =
    kind === "usb" ? "neutral" : kind === "tls_wifi" ? "success" : "warning";
  return <Badge tone={tone}>{t(transportLabels[kind])}</Badge>;
}

export function TransportTrustNotice({
  target,
  accepted,
  onAcceptedChange,
}: {
  target: DeviceTarget | null | undefined;
  accepted: boolean;
  onAcceptedChange: (accepted: boolean) => void;
}) {
  const { t } = useTranslation();
  if (!target || !requiresTransportOverride(target)) return null;

  return (
    <StatePanel title={t("devices.transportTrustTitle")} tone="warning">
      <p>
        {t("devices.transportTrustBody", {
          transport: t(transportLabels[target.transport_kind]),
        })}
      </p>
      <label className="mt-4 flex cursor-pointer items-start gap-3 rounded-md border border-amber-300/20 bg-amber-300/[0.06] p-3 text-anvil-100">
        <input
          type="checkbox"
          className="mt-1 h-4 w-4 accent-amber-300"
          checked={accepted}
          onChange={(event) => onAcceptedChange(event.currentTarget.checked)}
        />
        <span>{t("devices.transportTrustAcknowledge")}</span>
      </label>
    </StatePanel>
  );
}

/** Static by default. Set `live` only when the panel is inserted or updated in
 * response to an operation that assistive technology should announce. */
export function StatePanel({
  title,
  children,
  actions,
  tone = "neutral",
  live,
}: {
  title: string;
  children: ReactNode;
  actions?: ReactNode;
  tone?: "neutral" | "info" | "success" | "warning" | "danger";
  live?: "polite" | "assertive";
}) {
  return (
    <Card
      role={
        live === "assertive"
          ? "alert"
          : live === "polite"
            ? "status"
            : undefined
      }
      aria-atomic={live ? true : undefined}
      className={cn(
        "rounded-none border-0 border-s-2 py-3 ps-4 pe-3 shadow-none",
        tone === "neutral" && "border-s-white/20 bg-white/[0.025]",
        tone === "info" && "border-s-circuit-300/60 bg-circuit-950/20",
        tone === "success" && "border-s-emerald-300/60 bg-emerald-950/15",
        tone === "warning" && "border-s-amber-300/70 bg-amber-950/15",
        tone === "danger" && "border-s-red-300/70 bg-red-950/15",
      )}
    >
      <div className="flex gap-3">
        <StatusGlyph tone={tone} />
        <div className="min-w-0 flex-1">
          <h3 className="text-sm font-semibold text-anvil-50">{title}</h3>
          <div className="mt-1.5 text-sm text-anvil-300">{children}</div>
          {actions && (
            <div className="mt-4 flex flex-wrap gap-2">{actions}</div>
          )}
        </div>
      </div>
    </Card>
  );
}

export function SkeletonLine({ className }: { className?: string }) {
  return (
    <span
      className={cn(
        "block h-3 animate-pulse rounded-sm bg-white/[0.08]",
        className,
      )}
    />
  );
}

export function FieldInput({
  className,
  ...props
}: ComponentPropsWithoutRef<"input">) {
  return (
    <input
      className={cn(
        "h-10 rounded-[0.25rem] border border-white/[0.12] bg-black/15 px-3 text-sm text-anvil-50 outline-none transition",
        "placeholder:text-anvil-600 hover:border-white/20 focus:border-circuit-300/60 focus:ring-2 focus:ring-circuit-300/20",
        "disabled:cursor-not-allowed disabled:opacity-50",
        className,
      )}
      {...props}
    />
  );
}

export function FieldSelect({
  className,
  ...props
}: ComponentPropsWithoutRef<"select">) {
  return (
    <select
      className={cn(
        "h-10 rounded-[0.25rem] border border-white/[0.12] bg-black/15 px-3 text-sm text-anvil-50 outline-none transition",
        "hover:border-white/20 focus:border-circuit-300/60 focus:ring-2 focus:ring-circuit-300/20",
        "disabled:cursor-not-allowed disabled:opacity-50",
        className,
      )}
      {...props}
    />
  );
}

export function FieldTextArea({
  className,
  ...props
}: ComponentPropsWithoutRef<"textarea">) {
  return (
    <textarea
      className={cn(
        "w-full rounded-[0.25rem] border border-white/[0.12] bg-black/15 px-3 py-2 text-sm text-anvil-50 outline-none transition",
        "placeholder:text-anvil-600 hover:border-white/20 focus:border-circuit-300/60 focus:ring-2 focus:ring-circuit-300/20",
        "disabled:cursor-not-allowed disabled:opacity-50",
        className,
      )}
      {...props}
    />
  );
}

export function EmptyState({
  title,
  children,
  actions,
  className,
}: {
  title: string;
  children: ReactNode;
  actions?: ReactNode;
  className?: string;
}) {
  return (
    <div
      className={cn(
        "border-t border-white/10 bg-white/[0.018] px-5 py-7",
        className,
      )}
    >
      <div className="mx-auto max-w-xl text-center">
        <p className="text-sm font-semibold text-anvil-100">{title}</p>
        <div className="mt-2 text-sm leading-6 text-anvil-400">{children}</div>
        {actions && (
          <div className="mt-4 flex flex-wrap justify-center gap-2">
            {actions}
          </div>
        )}
      </div>
    </div>
  );
}

/**
 * Data-table accessibility baseline (IMP-73).
 *
 * The intended pattern is a **native semantic `<table>`** built from
 * `TableHeaderCell`/`TableCell`. Native table semantics give assistive tech
 * correct row/column navigation for free, so they are the default for every
 * fully-rendered data table (Network/Process inspectors, Debloat, Devices,
 * Fastboot, Profiles, Wireless, Journal, RecoveryBaseline, …).
 *
 * The ARIA **grid** pattern (`role="grid"`/`row"`/`gridcell"` +
 * `aria-rowcount`/`aria-rowindex`, plus `role="columnheader"` here) is used in
 * exactly one place — the **virtualized** `apps/PackageTable` — because its
 * lazy IntersectionObserver rendering keeps only visible rows in the DOM, so
 * `aria-rowcount` is required to announce the true total. Do NOT add
 * `role="grid"` to the plain tables: without a full keyboard grid-navigation
 * implementation it degrades, rather than improves, their accessibility.
 *
 * Sortable headers must expose `aria-sort` (see `devices/ProcessManager`).
 */
export function TableHeaderCell({
  children,
  align = "left",
  className,
  ...rest
}: {
  children: ReactNode;
  align?: "left" | "right";
  className?: string;
} & Omit<ComponentPropsWithoutRef<"th">, "className" | "children">) {
  return (
    <th
      className={cn(
        "px-3 py-2.5 text-xs font-medium text-anvil-400",
        align === "right" ? "text-end" : "text-start",
        // Grid-mode cells (role="columnheader") show a focus ring when they
        // become the roving tab stop; native table headers are unaffected.
        rest.role === "columnheader" &&
          "focus:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-circuit-300",
        className,
      )}
      {...rest}
    >
      {children}
    </th>
  );
}

export function TableCell({
  children,
  align = "left",
  className,
  ...rest
}: {
  children: ReactNode;
  align?: "left" | "right";
  className?: string;
} & Omit<ComponentPropsWithoutRef<"td">, "className" | "children">) {
  return (
    <td
      className={cn(
        "px-3 py-3 align-middle text-anvil-200",
        align === "right" && "text-end",
        rest.role === "gridcell" &&
          "focus:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-circuit-300",
        className,
      )}
      {...rest}
    >
      {children}
    </td>
  );
}

function StatusGlyph({
  tone,
}: {
  tone: "neutral" | "info" | "success" | "warning" | "danger";
}) {
  return (
    <span
      className={cn(
        "mt-0.5 grid h-5 w-5 shrink-0 place-items-center",
        tone === "neutral" && "text-anvil-400",
        tone === "info" && "text-circuit-200",
        tone === "success" && "text-emerald-200",
        tone === "warning" && "text-amber-200",
        tone === "danger" && "text-red-200",
      )}
      aria-hidden="true"
    >
      <svg
        viewBox="0 0 24 24"
        className="h-3.5 w-3.5"
        fill="none"
        stroke="currentColor"
        strokeLinecap="round"
        strokeLinejoin="round"
        strokeWidth="2"
      >
        {tone === "success" ? (
          <path d="m6 12 4 4 8-8" />
        ) : tone === "warning" ? (
          <>
            <path d="M12 8v5" />
            <path d="M12 17h.01" />
            <path d="M10.3 4.6 2.9 17.5A2 2 0 0 0 4.6 20h14.8a2 2 0 0 0 1.7-2.5L13.7 4.6a2 2 0 0 0-3.4 0Z" />
          </>
        ) : tone === "danger" ? (
          <>
            <path d="M12 8v5" />
            <path d="M12 17h.01" />
            <path d="M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18Z" />
          </>
        ) : (
          <>
            <path d="M12 16v-4" />
            <path d="M12 8h.01" />
            <path d="M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18Z" />
          </>
        )}
      </svg>
    </span>
  );
}
