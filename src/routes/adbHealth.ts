import type { AdbHealth, AdbRecoveryResult } from "../lib/tauri";

export const ADB_RECOVERY_COMMANDS = [
  "adb kill-server",
  "adb start-server",
  "adb reconnect offline",
] as const;

export function formatAdbDiagnostics({
  health,
  observedAt,
  recovery,
}: {
  health: AdbHealth | null;
  observedAt: string | null;
  recovery?: AdbRecoveryResult | null;
}): string {
  const lines = [
    "Droidsmith ADB diagnostics",
    `Observed: ${observedAt ?? "not available"}`,
    `Client version: ${health?.client_version ?? "not available"}`,
    `Server version: ${health?.server_version ?? "not available"}`,
    `Platform Tools status: ${health?.platform_tools.status ?? "not available"}`,
    `Platform Tools recommended: ${health?.platform_tools.recommended_version ?? "not available"}`,
    `Platform Tools policy reviewed: ${health?.platform_tools.policy_reviewed_on ?? "not available"}`,
    `Platform Tools rationale: ${health?.platform_tools.rationale ?? "not available"}`,
    `Platform Tools policy: ${health?.platform_tools.source_url ?? "not available"}`,
    `Server build: ${health?.server_build ?? "not available"}`,
    `USB backend: ${health?.usb_backend ?? "not available"}`,
    `mDNS enabled: ${formatOptionalBoolean(health?.mdns_enabled)}`,
    `mDNS backend: ${health?.mdns_backend ?? "not available"}`,
    `mDNS check: ${health?.mdns_check ?? "not available"}`,
    `Wi-Fi 2.0 readiness: ${health?.recommended_for_wifi_v2 ? "recommended platform-tools available" : `platform-tools ${health?.platform_tools.recommended_version ?? "recommended version"}+ required`}`,
    `Wi-Fi 2.0 discovery: ${health?.wifi_v2_state ?? "not available"}`,
    `Wi-Fi 2.0 devices: ${health?.wifi_v2_devices.join(", ") || "none detected"}`,
    `Warning: ${health?.warning ?? "none"}`,
  ];

  if (recovery) {
    lines.push(
      "",
      `Recovery operation: ${recovery.record.operation_id}`,
      `Recovery outcome: ${recovery.record.outcome}`,
      `Started: ${recovery.record.started_at}`,
      `Completed: ${recovery.record.completed_at ?? "not completed"}`,
      `Record: ${recovery.record_path}`,
      `Failure: ${recovery.record.failure ?? "none"}`,
      "Commands:",
      ...recovery.record.commands.map((args) => `  adb ${args.join(" ")}`),
    );
  }

  return `${lines.join("\n")}\n`;
}

function formatOptionalBoolean(value: boolean | null | undefined): string {
  return value == null ? "not available" : value ? "yes" : "no";
}
