// Shared inline SVG icons for the Devices route and its sub-panels (IMP-72:
// extracted verbatim from the former Devices.tsx god-file).

import { cn } from "../../lib/cn";

export function RefreshIcon({ spinning }: { spinning: boolean }) {
  return (
    <svg
      viewBox="0 0 24 24"
      className={cn("h-4 w-4", spinning && "animate-spin")}
      fill="none"
      stroke="currentColor"
      strokeLinecap="round"
      strokeLinejoin="round"
      strokeWidth="1.8"
      aria-hidden="true"
    >
      <path d="M20 11a8 8 0 0 0-14.8-4M4 4v5h5M4 13a8 8 0 0 0 14.8 4M20 20v-5h-5" />
    </svg>
  );
}

export function MoreIcon() {
  return (
    <svg
      viewBox="0 0 24 24"
      className="h-4 w-4"
      fill="currentColor"
      aria-hidden="true"
    >
      <circle cx="12" cy="5" r="1.5" />
      <circle cx="12" cy="12" r="1.5" />
      <circle cx="12" cy="19" r="1.5" />
    </svg>
  );
}

export function RecoveryIcon() {
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
      <path d="M5 7v5h5M19 17v-5h-5" />
      <path d="M7.1 17A8 8 0 0 0 19 12M16.9 7A8 8 0 0 0 5 12" />
    </svg>
  );
}

export function HealthCheckIcon({ healthy }: { healthy: boolean }) {
  return (
    <span
      className={cn(
        "flex h-5 w-5 shrink-0 items-center justify-center rounded-full border",
        healthy
          ? "border-emerald-300 text-emerald-300"
          : "border-amber-300 text-amber-300",
      )}
      aria-hidden="true"
    >
      {healthy ? (
        <svg
          viewBox="0 0 20 20"
          className="h-3 w-3"
          fill="none"
          stroke="currentColor"
          strokeLinecap="round"
          strokeLinejoin="round"
          strokeWidth="2"
        >
          <path d="m5 10 3 3 7-7" />
        </svg>
      ) : (
        <span className="h-1.5 w-1.5 rounded-full bg-current" />
      )}
    </span>
  );
}

export function SelectionIcon({ selected }: { selected: boolean }) {
  if (!selected) {
    return (
      <span
        className="h-5 w-5 rounded-full border border-white/[0.14]"
        aria-hidden="true"
      />
    );
  }

  return (
    <span
      className="flex h-6 w-6 items-center justify-center rounded-full bg-circuit-300 text-anvil-950 shadow-sm"
      aria-hidden="true"
    >
      <svg
        viewBox="0 0 20 20"
        className="h-3.5 w-3.5"
        fill="none"
        stroke="currentColor"
        strokeLinecap="round"
        strokeLinejoin="round"
        strokeWidth="2"
      >
        <path d="m5 10 3 3 7-7" />
      </svg>
    </span>
  );
}
