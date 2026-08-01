import type { ScrcpyCapabilities, ScrcpySession } from "../lib/tauri";

export const IGNORE_ENCODER_CONSTRAINTS_FLAG =
  "--ignore-video-encoder-constraints";

export function canRetryEncoderConstraints(
  session: ScrcpySession,
  capabilities: ScrcpyCapabilities,
): boolean {
  return (
    capabilities.supports_ignore_video_encoder_constraints &&
    session.state === "exited" &&
    session.exit_reason === "encoder_constraint_failed" &&
    !session.args.includes(IGNORE_ENCODER_CONSTRAINTS_FLAG)
  );
}
