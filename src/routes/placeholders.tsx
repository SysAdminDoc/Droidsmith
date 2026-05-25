import { PaneHeader, PlaceholderBody } from "./common";

export function AppsRoute() {
  return (
    <>
      <PaneHeader
        title="Apps"
        milestone="R-020"
        description="Installed apps with real labels and icons, filters for user/system/disabled, and bulk-select actions feeding the queue."
      />
      <PlaceholderBody
        bullets={[
          "Two-pass union of `pm list packages -e` and `-d` (already wired in the Rust backend).",
          "Filters: user vs system, enabled vs disabled, has-data, recently-installed.",
          "Multi-select drives the bulk action queue (R-022) with a preview-diff confirmation.",
          "Drag-and-drop APK = silent install, like scrcpy 4 (R-023).",
          "Per-app icon and label resolution via pure-Rust APK parsing (F-NEW-09).",
        ]}
        commands={[
          {
            name: "list_packages",
            sig: "(serial, filter) -> AppPackage[]",
            ready: true,
          },
          {
            name: "plan_action",
            sig: "(request) -> PlannedAction",
            ready: true,
          },
          {
            name: "apply_action",
            sig: "(plan) -> JournalEntry",
            ready: true,
          },
          {
            name: "extract_apk",
            sig: "(serial, package, dest) -> path",
            ready: false,
          },
        ]}
      />
    </>
  );
}

export function DebloatRoute() {
  return (
    <>
      <PaneHeader
        title="Debloat"
        milestone="R-033"
        description="Pick a community pack, preview the diff, apply atomically, undo from the journal."
      />
      <PlaceholderBody
        bullets={[
          "Pack picker filters by detected manufacturer / Android version (matches PackTargets).",
          "Preview-diff shows synthesized adb commands AND vendor-quirk warnings (R-034 already wired on the Rust side).",
          "Apply runs as a transaction-with-resume — a failure pauses the batch and surfaces the explanatory error.",
          "Undo button on every row, backed by the per-device journal.",
          "Optional UAD-NG list import (R-036) attributes upstream cleanly.",
        ]}
        commands={[
          {
            name: "packs::load",
            sig: "(path) -> Pack",
            ready: true,
          },
          {
            name: "packs::lint",
            sig: "(pack) -> Vec<reason>",
            ready: true,
          },
          {
            name: "explain_failure",
            sig: "(req) -> Option<Quirk>",
            ready: true,
          },
          {
            name: "list_packs",
            sig: "() -> Vec<Pack>",
            ready: false,
          },
        ]}
      />
    </>
  );
}

export function MirrorRoute() {
  return (
    <>
      <PaneHeader
        title="Mirror"
        milestone="R-040"
        description="scrcpy 4.x-driven screen mirror with audio, recording, and drag-APK-installs."
      />
      <PlaceholderBody
        bullets={[
          "Spawns the bundled scrcpy sidecar with per-device defaults (60fps, audio on, max-size matched).",
          "Drag an APK onto the window = silent install (scrcpy native behaviour).",
          "Drag any other file = push to /sdcard/Download (configurable via --push-target).",
          "Record button toggles `--record session.mp4`.",
          "Per-device session prefs persist (bitrate, max-size, audio, codec).",
        ]}
        commands={[
          {
            name: "scrcpy_launch",
            sig: "(serial, options) -> ()",
            ready: false,
          },
          {
            name: "scrcpy_record",
            sig: "(serial, output) -> ()",
            ready: false,
          },
        ]}
      />
    </>
  );
}

export function ConsoleRoute() {
  return (
    <>
      <PaneHeader
        title="Console"
        milestone="R-050"
        description="Multi-tab adb shell with history, favourites, syntax highlighting."
      />
      <PlaceholderBody
        bullets={[
          "Each tab targets one (serial, command). Persists between sessions.",
          "Favourites slot for the 8 commands you actually use (dumpsys battery, top -m 5, etc.).",
          "Output is streamed (Tauri events), so logcat-style commands don't wedge the UI.",
          "Copy-as-snippet button so bug reports come with reproducible commands.",
        ]}
        commands={[
          {
            name: "shell_run_oneshot",
            sig: "(serial, argv) -> String",
            ready: true,
          },
          {
            name: "shell_stream_start",
            sig: "(serial, argv) -> stream_id",
            ready: false,
          },
        ]}
      />
    </>
  );
}

export function LogcatRoute() {
  return (
    <>
      <PaneHeader
        title="Logcat"
        milestone="R-051"
        description="Live tail with tag / pid / level filters and grep."
      />
      <PlaceholderBody
        bullets={[
          "Streams `adb logcat -v threadtime` per device.",
          "Tag/pid/level filters compile into a single grep so the stream stays cheap.",
          "Pause / Clear / Save-to-file. The journal links to saved log slices.",
          "Sentry-style line grouping for repeat events.",
        ]}
        commands={[
          {
            name: "logcat_start",
            sig: "(serial, filters) -> stream_id",
            ready: false,
          },
          {
            name: "logcat_stop",
            sig: "(stream_id) -> ()",
            ready: false,
          },
        ]}
      />
    </>
  );
}

export function FastbootRoute() {
  return (
    <>
      <PaneHeader
        title="Fastboot"
        milestone="R-052"
        description="Fastboot mode, partition inspector, slot management."
      />
      <PlaceholderBody
        bullets={[
          "Detects devices in fastboot/bootloader mode (separate from `adb devices`).",
          "Reads partition table + current slot via `fastboot getvar all`.",
          "Lock/unlock warnings spelled out before any destructive action.",
          "Bundled fastboot sidecar landed with R-010 — same fetch script as adb.",
        ]}
        commands={[
          {
            name: "fastboot_devices",
            sig: "() -> Device[]",
            ready: false,
          },
          {
            name: "fastboot_getvar",
            sig: "(serial, key) -> String",
            ready: false,
          },
        ]}
      />
    </>
  );
}
