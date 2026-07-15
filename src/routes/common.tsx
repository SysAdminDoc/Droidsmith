import type { ComponentPropsWithoutRef, ReactNode } from "react";
import { useState } from "react";
import { useTranslation } from "react-i18next";

import { cn } from "../lib/cn";
import {
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
    <header className="border-b border-white/10 pb-6 sm:pb-7">
      <div className="flex flex-col gap-5 sm:flex-row sm:items-start sm:justify-between">
        <div className="min-w-0">
          <div className="flex flex-wrap items-center gap-3">
            <h2 className="text-2xl font-semibold tracking-tight text-anvil-50 sm:text-3xl">
              {title}
            </h2>
            <MilestoneBadge milestone={milestone} />
          </div>
          <p className="mt-3 max-w-3xl text-sm leading-6 text-anvil-300">
            {description}
          </p>
          {meta && <div className="mt-4">{meta}</div>}
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

export function PlaceholderBody({
  bullets,
  commands,
}: {
  bullets: string[];
  commands: { name: string; sig: string; ready: boolean }[];
}) {
  const readyCount = commands.filter((command) => command.ready).length;

  return (
    <section className="mt-6 grid max-w-6xl gap-4 xl:grid-cols-[minmax(0,1fr)_minmax(340px,0.85fr)]">
      <Card className="p-5">
        <div className="flex items-start justify-between gap-4">
          <div>
            <h3 className="text-sm font-semibold text-anvil-50">
              Planned workflow
            </h3>
            <p className="mt-1 text-xs leading-5 text-anvil-400">
              The pane is staged so contributors can see the product behavior
              before the final controls land.
            </p>
          </div>
          <Badge tone="info">{bullets.length} steps</Badge>
        </div>
        <ol className="mt-5 space-y-3">
          {bullets.map((bullet, index) => (
            <li key={bullet} className="flex gap-3 text-sm leading-6">
              <span className="mt-0.5 flex h-6 w-6 shrink-0 items-center justify-center rounded-md border border-white/10 bg-white/[0.04] font-mono text-[11px] text-anvil-300">
                {index + 1}
              </span>
              <span className="text-anvil-200">{bullet}</span>
            </li>
          ))}
        </ol>
      </Card>

      <Card className="p-5">
        <div className="flex items-start justify-between gap-4">
          <div>
            <h3 className="text-sm font-semibold text-anvil-50">
              Backend readiness
            </h3>
            <p className="mt-1 text-xs leading-5 text-anvil-400">
              IPC commands already exposed by the Rust side are marked ready.
            </p>
          </div>
          <Badge tone={readyCount === commands.length ? "success" : "neutral"}>
            {readyCount}/{commands.length} ready
          </Badge>
        </div>

        <ul className="mt-5 divide-y divide-white/10" aria-label="Commands">
          {commands.map((command) => (
            <li
              key={command.name}
              className="grid gap-2 py-3 first:pt-0 last:pb-0 sm:grid-cols-[auto_minmax(0,1fr)] sm:items-center"
            >
              <Badge tone={command.ready ? "success" : "neutral"}>
                {command.ready ? "Ready" : "Queued"}
              </Badge>
              <p className="min-w-0 font-mono text-xs leading-5">
                <span className="text-anvil-50">{command.name}</span>
                <span className="text-anvil-400">{command.sig}</span>
              </p>
            </li>
          ))}
        </ul>
      </Card>
    </section>
  );
}

export function Card({
  children,
  className,
  ...props
}: ComponentPropsWithoutRef<"div">) {
  return (
    <div
      className={cn(
        "rounded-lg border border-white/10 bg-anvil-900/70 shadow-glow backdrop-blur",
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
        "inline-flex items-center justify-center gap-2 rounded-md font-medium transition duration-150",
        "focus:outline-none focus-visible:ring-2 focus-visible:ring-circuit-300 focus-visible:ring-offset-2 focus-visible:ring-offset-anvil-950",
        "disabled:cursor-not-allowed disabled:opacity-50",
        size === "sm" ? "min-h-8 px-2.5 text-xs" : "min-h-9 px-3.5 text-sm",
        variant === "primary" &&
          "bg-circuit-300 text-anvil-950 shadow-[0_0_22px_rgba(34,211,238,0.22)] hover:bg-circuit-200 active:bg-circuit-400",
        variant === "secondary" &&
          "border border-white/10 bg-white/[0.07] text-anvil-50 hover:border-white/20 hover:bg-white/[0.11] active:bg-white/[0.14]",
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
            setError(cause instanceof Error ? cause.message : String(cause)),
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
    <Card className="p-4">
      <h3 className="text-sm font-semibold text-anvil-50">
        {t("common.selectDevice")}
      </h3>
      <div className="mt-3 flex flex-wrap gap-2">
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
    </Card>
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
        "inline-flex min-h-6 items-center rounded-md border px-2 py-0.5 text-xs font-medium",
        tone === "neutral" && "border-white/10 bg-white/[0.05] text-anvil-300",
        tone === "info" &&
          "border-circuit-300/25 bg-circuit-300/10 text-circuit-100",
        tone === "success" &&
          "border-emerald-300/25 bg-emerald-300/10 text-emerald-100",
        tone === "warning" &&
          "border-amber-300/25 bg-amber-300/10 text-amber-100",
        tone === "danger" && "border-red-300/25 bg-red-300/10 text-red-100",
        className,
      )}
    >
      {children}
    </span>
  );
}

export function MilestoneBadge({ milestone }: { milestone: string }) {
  const { t } = useTranslation();

  return (
    <Badge tone="neutral" className="font-mono uppercase tracking-[0.08em]">
      {t("common.roadmap", { milestone })}
    </Badge>
  );
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

export function StatePanel({
  title,
  children,
  actions,
  tone = "neutral",
}: {
  title: string;
  children: ReactNode;
  actions?: ReactNode;
  tone?: "neutral" | "info" | "success" | "warning" | "danger";
}) {
  return (
    <Card
      role={tone === "danger" ? "alert" : "status"}
      className={cn(
        "p-5",
        tone === "info" && "border-circuit-300/20 bg-circuit-950/35",
        tone === "success" && "border-emerald-300/20 bg-emerald-950/20",
        tone === "warning" && "border-amber-300/20 bg-amber-950/20",
        tone === "danger" && "border-red-300/20 bg-red-950/20",
      )}
    >
      <div className="flex gap-4">
        <StatusGlyph tone={tone} />
        <div className="min-w-0 flex-1">
          <h3 className="text-sm font-semibold text-anvil-50">{title}</h3>
          <div className="mt-2 text-sm leading-6 text-anvil-300">
            {children}
          </div>
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
        "h-9 rounded-md border border-white/10 bg-white/[0.06] px-3 text-sm text-anvil-50 outline-none transition",
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

export function TableHeaderCell({
  children,
  align = "left",
  className,
}: {
  children: ReactNode;
  align?: "left" | "right";
  className?: string;
}) {
  return (
    <th
      className={cn(
        "px-4 py-3 text-[11px] font-semibold uppercase tracking-[0.08em] text-anvil-400",
        align === "right" ? "text-right" : "text-left",
        className,
      )}
    >
      {children}
    </th>
  );
}

export function TableCell({
  children,
  align = "left",
  className,
}: {
  children: ReactNode;
  align?: "left" | "right";
  className?: string;
}) {
  return (
    <td
      className={cn(
        "px-4 py-4 align-middle text-anvil-200",
        align === "right" && "text-right",
        className,
      )}
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
        "mt-0.5 grid h-7 w-7 shrink-0 place-items-center rounded-md border",
        tone === "neutral" && "border-white/10 bg-white/[0.05] text-anvil-300",
        tone === "info" &&
          "border-circuit-300/25 bg-circuit-300/10 text-circuit-100",
        tone === "success" &&
          "border-emerald-300/25 bg-emerald-300/10 text-emerald-100",
        tone === "warning" &&
          "border-amber-300/25 bg-amber-300/10 text-amber-100",
        tone === "danger" && "border-red-300/25 bg-red-300/10 text-red-100",
      )}
      aria-hidden="true"
    >
      <svg
        viewBox="0 0 24 24"
        className="h-4 w-4"
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
