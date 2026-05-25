import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

type Heartbeat = {
  version: string;
  adb_resolved: string | null;
};

export default function App() {
  const [hb, setHb] = useState<Heartbeat | null>(null);
  const [err, setErr] = useState<string | null>(null);

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
        <nav className="mt-6 space-y-1 text-sm">
          <NavStub label="Devices" />
          <NavStub label="Apps" />
          <NavStub label="Debloat" />
          <NavStub label="Mirror" />
          <NavStub label="Console" />
          <NavStub label="Logcat" />
          <NavStub label="Fastboot" />
        </nav>
      </aside>
      <main className="flex flex-1 flex-col items-center justify-center p-8">
        <div className="max-w-md text-center">
          <h2 className="text-2xl font-semibold">Forge in progress</h2>
          <p className="mt-2 text-sm text-anvil-300">
            R-002 scaffold. The shell is here; features land per the
            ROADMAP. Track progress in{" "}
            <code className="rounded bg-anvil-800 px-1 py-0.5 font-mono text-xs">
              ROADMAP.md
            </code>
            .
          </p>
          <div className="mt-6 rounded-lg border border-anvil-800 bg-anvil-900 p-4 text-left font-mono text-xs">
            <div className="text-anvil-400">heartbeat</div>
            {err && <div className="text-red-400">error: {err}</div>}
            {hb && (
              <>
                <div>version: {hb.version}</div>
                <div>
                  adb:{" "}
                  {hb.adb_resolved ?? (
                    <span className="text-anvil-400">not detected</span>
                  )}
                </div>
              </>
            )}
          </div>
        </div>
      </main>
    </div>
  );
}

function NavStub({ label }: { label: string }) {
  return (
    <div
      className="cursor-not-allowed rounded px-2 py-1.5 text-anvil-400 hover:bg-anvil-900"
      title="Not implemented yet"
    >
      {label}
    </div>
  );
}
