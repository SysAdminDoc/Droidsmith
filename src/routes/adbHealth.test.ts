import { describe, expect, it } from "vitest";

import type { AdbHealth, AdbRecoveryResult } from "../lib/tauri";
import { formatAdbDiagnostics } from "./adbHealth";

const health: AdbHealth = {
  server_status_supported: true,
  client_version: "37.0.0",
  server_version: "37.0.0",
  server_build: "123456",
  usb_backend: "NATIVE",
  mdns_backend: "LIBADBMDNS",
  mdns_enabled: true,
  mdns_check: "mdns daemon version [Openscreen discovery 0.0.0]",
  burst_mode: true,
  recommended_for_wifi_v2: true,
  wifi_v2_state: "supported",
  wifi_v2_devices: ["Pixel 10"],
  warning: null,
};

describe("formatAdbDiagnostics", () => {
  it("renders shareable health and a durable recovery reference", () => {
    const recovery: AdbRecoveryResult = {
      record_path: "C:\\AppData\\Droidsmith\\host-operations.jsonl",
      record: {
        schema_version: 1,
        operation_id: "adb-recovery-test",
        operation: "adb_server_recovery",
        confirmation_source: "devices_health_review",
        outcome: "succeeded",
        started_at: "2026-07-14T18:00:00Z",
        completed_at: "2026-07-14T18:00:02Z",
        commands: [["kill-server"], ["start-server"], ["reconnect", "offline"]],
        health_before: health,
        health_after: health,
        failure: null,
      },
    };

    const output = formatAdbDiagnostics({
      health,
      observedAt: "2026-07-14T18:00:02Z",
      recovery,
    });
    expect(output).toContain("mDNS backend: LIBADBMDNS");
    expect(output).toContain("Wi-Fi 2.0 devices: Pixel 10");
    expect(output).toContain("Recovery outcome: succeeded");
    expect(output).toContain("adb reconnect offline");
    expect(output).not.toContain("undefined");
  });
});
