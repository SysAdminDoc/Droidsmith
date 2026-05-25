import { PaneHeader, PlaceholderBody } from "./common";

export function AppsRoute() {
  return (
    <>
      <PaneHeader
        title="Apps"
        milestone="R-020"
        description="Review installed packages with readable labels, precise filters, and a bulk action queue that makes every uninstall, disable, or extract step previewable."
      />
      <PlaceholderBody
        bullets={[
          "Merge enabled and disabled package scans so the list never hides apps that matter.",
          "Filter quickly by user, system, disabled, data-bearing, and recently installed packages.",
          "Send multi-select choices into a preview queue before any bulk action is applied.",
          "Install APKs from drag and drop when the silent install path is available.",
          "Resolve package labels and icons through the Rust APK parser instead of raw package names.",
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
        description="Choose an OEM-aware pack, understand the risk before applying it, and keep every debloat change reversible through the journal."
      />
      <PlaceholderBody
        bullets={[
          "Filter packs by detected manufacturer and Android version before the user chooses one.",
          "Show the generated ADB commands beside vendor-specific quirk warnings.",
          "Apply changes as a resumable transaction so failures pause instead of burying context.",
          "Attach undo controls to journal rows for device-specific recovery.",
          "Import UAD-NG lists with clean upstream attribution.",
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
        description="Launch polished scrcpy sessions with sensible defaults for viewing, recording, file drop, and APK install workflows."
      />
      <PlaceholderBody
        bullets={[
          "Launch the bundled scrcpy sidecar with per-device defaults for frame rate, audio, and max size.",
          "Treat dropped APK files as install intents when scrcpy supports the path.",
          "Push non-APK files to the configured device download location.",
          "Make recording an explicit session state with a clear output target.",
          "Persist bitrate, max size, audio, and codec preferences per device.",
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
        description="Run shell commands in a focused workspace with tabs, favorites, streamed output, and copyable diagnostic snippets."
      />
      <PlaceholderBody
        bullets={[
          "Persist each tab by target device and command so sessions can resume cleanly.",
          "Keep a compact favorite shelf for frequently repeated diagnostics.",
          "Stream output through Tauri events so long-running commands stay responsive.",
          "Copy commands and output as reproducible report snippets.",
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
        description="Tail device logs with fast filters, pause and save controls, and grouping that makes repeated events easier to read."
      />
      <PlaceholderBody
        bullets={[
          "Stream `adb logcat -v threadtime` per selected device.",
          "Compile tag, pid, level, and text filters into one cheap device-side query.",
          "Expose pause, clear, and save-to-file controls without hiding live status.",
          "Group repeated lines so noisy errors stay scannable.",
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
        description="Inspect bootloader devices, partition state, and slots with clear safety copy before any destructive command is enabled."
      />
      <PlaceholderBody
        bullets={[
          "Detect fastboot and bootloader-mode devices separately from regular ADB targets.",
          "Read partition table and current slot data through `fastboot getvar all`.",
          "Spell out lock and unlock risk before enabling destructive actions.",
          "Use the bundled fastboot sidecar once the shared platform-tools fetch path is active.",
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
