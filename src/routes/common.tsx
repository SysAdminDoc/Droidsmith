import type { ReactNode } from "react";

/** Standard pane header. Every route uses this for visual consistency. */
export function PaneHeader({
  title,
  milestone,
  description,
}: {
  title: string;
  milestone: string;
  description: string;
}) {
  return (
    <>
      <header className="mb-6 flex items-baseline justify-between gap-4">
        <h2 className="text-2xl font-semibold">{title}</h2>
        <p className="text-xs text-anvil-300">
          Coming in{" "}
          <code className="rounded bg-anvil-800 px-1.5 py-0.5 font-mono">
            {milestone}
          </code>
        </p>
      </header>
      <p className="max-w-prose text-sm text-anvil-200">{description}</p>
    </>
  );
}

/** Block shown for not-yet-implemented panes. Lists the planned actions
 *  and the IPC commands the Rust side already exposes so contributors
 *  can see how far each pane is from done. */
export function PlaceholderBody({
  bullets,
  commands,
}: {
  bullets: string[];
  commands: { name: string; sig: string; ready: boolean }[];
}) {
  return (
    <section className="mt-6 grid max-w-3xl gap-6 lg:grid-cols-2">
      <div>
        <h3 className="mb-2 text-sm font-semibold text-anvil-50">
          Planned behaviour
        </h3>
        <ul className="list-inside list-disc space-y-1 text-sm text-anvil-200">
          {bullets.map((b) => (
            <li key={b}>{b}</li>
          ))}
        </ul>
      </div>
      <div>
        <h3 className="mb-2 text-sm font-semibold text-anvil-50">
          Backend commands
        </h3>
        <ul className="space-y-1 font-mono text-xs">
          {commands.map((c) => (
            <li
              key={c.name}
              className={c.ready ? "text-anvil-100" : "text-anvil-300"}
            >
              <span
                className={
                  c.ready
                    ? "rounded bg-emerald-900/40 px-1.5 py-0.5 text-emerald-200"
                    : "rounded bg-anvil-800 px-1.5 py-0.5 text-anvil-300"
                }
              >
                {c.ready ? "READY" : "TODO "}
              </span>{" "}
              <span className="text-anvil-50">{c.name}</span>
              <span className="text-anvil-300">{c.sig}</span>
            </li>
          ))}
        </ul>
      </div>
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
      className={`rounded-lg border border-anvil-800 bg-anvil-900 p-4 ${className}`}
    >
      {children}
    </div>
  );
}
