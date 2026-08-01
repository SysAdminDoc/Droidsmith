import { describe, expect, it } from "vitest";

import type { ScrcpyCapabilities, ScrcpySession } from "../lib/tauri";

import {
  canRetryEncoderConstraints,
  IGNORE_ENCODER_CONSTRAINTS_FLAG,
} from "./mirrorRecovery";

const session = {
  id: 7,
  serial: "DEVICE123",
  pid: 1234,
  args: ["-s", "DEVICE123", "--max-size", "1280"],
  started_at: "2026-07-29T12:00:00Z",
  state: "exited",
  exit_code: 1,
  exit_reason: "encoder_constraint_failed",
  stderr_tail: "[server] ERROR: MediaCodec rejected video size 1280x720",
} satisfies ScrcpySession;

const capabilities = {
  supports_ignore_video_encoder_constraints: true,
} as ScrcpyCapabilities;

describe("scrcpy encoder-constraint recovery", () => {
  it("offers a retry only for the recognized 4.1+ failure", () => {
    expect(canRetryEncoderConstraints(session, capabilities)).toBe(true);
    expect(
      canRetryEncoderConstraints(
        { ...session, exit_reason: "encoder_failed" },
        capabilities,
      ),
    ).toBe(false);
    expect(
      canRetryEncoderConstraints(session, {
        ...capabilities,
        supports_ignore_video_encoder_constraints: false,
      }),
    ).toBe(false);
  });

  it("never loops after the reviewed override has already been used", () => {
    expect(
      canRetryEncoderConstraints(
        {
          ...session,
          args: [...session.args, IGNORE_ENCODER_CONSTRAINTS_FLAG],
        },
        capabilities,
      ),
    ).toBe(false);
  });
});
