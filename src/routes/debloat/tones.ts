import type { CompatibilityStatus } from "../../lib/tauri";

/// Map a pack/check compatibility status to a Badge tone. Shared across the
/// pack picker, preview, and compatibility panels.
export function compatibilityTone(
  status: CompatibilityStatus,
): "success" | "warning" | "danger" {
  switch (status) {
    case "compatible":
      return "success";
    case "unknown":
      return "warning";
    case "mismatch":
      return "danger";
  }
}
