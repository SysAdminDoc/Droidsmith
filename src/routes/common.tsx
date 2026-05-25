import type { ComponentPropsWithoutRef, ReactNode } from "react";

import { cn } from "../lib/cn";

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
    <header className="border-b border-white/10 pb-6">
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
          <div className="flex shrink-0 flex-wrap items-center gap-2">
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
              <span className="mt-0.5 flex h-6 w-6 shrink-0 items-center justify-center rounded-full border border-white/10 bg-white/[0.04] font-mono text-[11px] text-anvil-300">
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
  className = "",
}: {
  children: ReactNode;
  className?: string;
}) {
  return (
    <div
      className={cn(
        "rounded-lg border border-white/10 bg-anvil-900/70 shadow-glow backdrop-blur",
        className || "p-4",
      )}
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
        "disabled:cursor-not-allowed disabled:opacity-45",
        size === "sm" ? "h-8 px-2.5 text-xs" : "h-9 px-3.5 text-sm",
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
        "inline-flex min-h-6 items-center rounded-full border px-2 py-0.5 text-xs font-medium",
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
  return (
    <Badge tone="neutral" className="font-mono uppercase tracking-[0.08em]">
      Roadmap {milestone}
    </Badge>
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
      className={cn(
        "p-5",
        tone === "info" && "border-circuit-300/20 bg-circuit-950/35",
        tone === "success" && "border-emerald-300/20 bg-emerald-950/20",
        tone === "warning" && "border-amber-300/20 bg-amber-950/20",
        tone === "danger" && "border-red-300/20 bg-red-950/20",
      )}
    >
      <div className="flex gap-4">
        <span
          className={cn(
            "mt-1 h-2.5 w-2.5 shrink-0 rounded-full ring-4",
            tone === "neutral" && "bg-anvil-300 ring-anvil-300/10",
            tone === "info" && "bg-circuit-300 ring-circuit-300/10",
            tone === "success" && "bg-emerald-300 ring-emerald-300/10",
            tone === "warning" && "bg-amber-300 ring-amber-300/10",
            tone === "danger" && "bg-red-300 ring-red-300/10",
          )}
          aria-hidden="true"
        />
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
        "block h-3 animate-pulse rounded-full bg-white/[0.08]",
        className,
      )}
    />
  );
}
