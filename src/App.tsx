import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

type ResolveSource =
  | "path"
  | "android_home"
  | "android_studio"
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

const NAV_ITEMS: Array<{ label: string; milestone: string }> = [
  { label: "Devices", milestone: "R-012" },
  { label: "Apps", milestone: "R-020" },
  { label: "Debloat", milestone: "R-033" },
  { label: "Mirror", milestone: "R-040" },
  { label: "Console", milestone: "R-050" },
  { label: "Logcat", milestone: "R-051" },
  { label: "Fastboot", milestone: "R-052" },
];

export default function App() {
  const [hb, setHb] = useState<Heartbeat | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [active, setActive] = useState<string>("Devices");

  useEffect(() => {
    invoke<Heartbeat>("heartbeat")
      .then(setHb)
      .catch((e) => setErr(String(e)));
  }, []);

  return (
    <div className="flex h-full">
      <aside className="w-56 border-r border-anvil-800 bg-anvil-950 p-4">
        <h1 className="font-mono text-lg font-semibold tracking-tight">
          Droidsmith
        </h1>
        <p className="mt-1 text-xs text-anvil-400">
          v{hb?.version ?? "…"}
        </p>
        <nav className="mt-6 space-y-1 text-sm" aria-label="Primary">
          {NAV_ITEMS.map((item) => (
            <NavStub
              key={item.label}
              label={item.label}
              milestone={item.milestone}
              active={active === item.label}
              onActivate={() => setActive(item.label)}
            />
          ))}
        </nav>
      </aside>
      <main className="flex flex-1 flex-col p-8">
        <header className="mb-6 flex items-baseline justify-between">
          <h2 className="text-2xl font-semibold">{active}</h2>
          <p className="text-xs text-anvil-400">
            Coming in milestone{" "}
            <code className="rounded bg-anvil-800 px-1.5 py-0.5 font-mono">
              {NAV_ITEMS.find((i) => i.label === active)?.milestone}
            </code>
          </p>
        </header>
        <p className="max-w-prose text-sm text-anvil-300">
          The shell is here; features land per the{" "}
          <code className="rounded bg-anvil-800 px-1 py-0.5 font-mono text-xs">
            ROADMAP.md
          </code>
          . Until those milestones close, this pane is a placeholder.
        </p>
        <section className="mt-8 max-w-lg">
          <h3 className="mb-2 text-sm font-semibold text-anvil-100">
            Heartbeat
          </h3>
          <div className="rounded-lg border border-anvil-800 bg-anvil-900 p-4 font-mono text-xs">
            {err && <div className="text-red-400">error: {err}</div>}
            {!err && !hb && (
              <div className="text-anvil-400">loading…</div>
            )}
            {hb && (
              <dl className="grid grid-cols-[8rem_1fr] gap-y-1">
                <KV k="droidsmith" v={`v${hb.version}`} />
                <KV
                  k="os"
                  v={`${hb.os.family} ${hb.os.version} (${hb.os.arch})`}
                />
                <KV k="tauri" v={`v${hb.tauri_version}`} />
                <KV k="rust msrv" v={hb.rust_version} />
                <KV k="app data" v={hb.app_data_dir ?? "—"} />
                <KV
                  k="adb"
                  v={
                    hb.adb.path
                      ? `${hb.adb.path}${hb.adb.version ? ` — ${hb.adb.version}` : ""}`
                      : "not detected (bundle landing in R-010)"
                  }
                />
                <KV k="adb source" v={hb.adb.source.replace(/_/g, " ")} />
              </dl>
            )}
          </div>
        </section>
      </main>
    </div>
  );
}

function NavStub({
  label,
  milestone,
  active,
  onActivate,
}: {
  label: string;
  milestone: string;
  active: boolean;
  onActivate: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onActivate}
      aria-current={active ? "page" : undefined}
      className={`flex w-full items-center justify-between rounded px-2 py-1.5 text-left transition-colors ${
        active
          ? "bg-anvil-800 text-anvil-50"
          : "text-anvil-200 hover:bg-anvil-900"
      }`}
      title={`Coming in ${milestone}`}
    >
      <span>{label}</span>
      <span className="font-mono text-[10px] text-anvil-400">
        {milestone}
      </span>
    </button>
  );
}

function KV({ k, v }: { k: string; v: string }) {
  return (
    <>
      <dt className="text-anvil-400">{k}</dt>
      <dd className="break-all text-anvil-100">{v}</dd>
    </>
  );
}
