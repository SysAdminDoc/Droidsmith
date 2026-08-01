import { describe, expect, it } from "vitest";

import { deviceStateHelpKey, deviceStateTone, formatLinkSpeed } from "./common";

describe("structured device presentation", () => {
  it("keeps detached, no-permission, and rescue states distinct", () => {
    expect(deviceStateHelpKey("detached")).toBe("devices.stateHelp.detached");
    expect(deviceStateHelpKey("no_permissions")).toBe(
      "devices.stateHelp.no_permissions",
    );
    expect(deviceStateHelpKey("rescue")).toBe("devices.stateHelp.rescue");
    expect(deviceStateTone("detached")).toBe("warning");
    expect(deviceStateTone("no_permissions")).toBe("danger");
    expect(deviceStateTone("rescue")).toBe("info");
  });

  it("does not attach an error explanation to an actionable device", () => {
    expect(deviceStateHelpKey("device")).toBeNull();
  });

  it("formats ADB link speeds as SI bits per second", () => {
    expect(formatLinkSpeed(5_000_000_000)).toBe("5.0 Gbps");
    expect(formatLinkSpeed(480_000_000)).toBe("480.0 Mbps");
    expect(formatLinkSpeed(0)).toBe("0 bps");
  });
});
