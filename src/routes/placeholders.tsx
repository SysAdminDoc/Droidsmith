import { PaneHeader, PlaceholderBody } from "./common";

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
