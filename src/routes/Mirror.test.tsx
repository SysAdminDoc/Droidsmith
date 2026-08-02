import { describe, expect, it } from "vitest";

import {
  canRetryEncoderConstraints,
  IGNORE_ENCODER_CONSTRAINTS_FLAG,
} from "./mirrorRecovery";

describe("Mirror route recovery guard", () => {
  it("only offers the encoder retry after the matching failed session", () => {
    const capabilities = {
      supports_ignore_video_encoder_constraints: true,
    } as never;
    expect(
      canRetryEncoderConstraints(
        {
          state: "exited",
          exit_reason: "encoder_constraint_failed",
          args: [],
        } as never,
        capabilities,
      ),
    ).toBe(true);
    expect(
      canRetryEncoderConstraints(
        {
          state: "running",
          exit_reason: "encoder_constraint_failed",
          args: [],
        } as never,
        capabilities,
      ),
    ).toBe(false);
    expect(
      canRetryEncoderConstraints(
        {
          state: "exited",
          exit_reason: "encoder_constraint_failed",
          args: [IGNORE_ENCODER_CONSTRAINTS_FLAG],
        } as never,
        capabilities,
      ),
    ).toBe(false);
  });
});
