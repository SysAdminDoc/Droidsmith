import { PaneHeader, PlaceholderBody } from "./common";

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
