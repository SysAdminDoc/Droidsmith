import { describe, expect, it } from "vitest";

import { deviceIdentityKey } from "./deviceIdentity";

describe("deviceIdentityKey", () => {
  it("matches the literal pinned in device_identity.rs", () => {
    // The Rust test `canonical_form_is_pinned_and_mirrored_by_the_renderer_helper`
    // asserts this exact string. Both must move together or every device's
    // settings scope silently splits in two.
    expect(
      deviceIdentityKey({
        serial: "R5CT60ZQR4M",
        build_fingerprint: "google/panther/panther:16/BP1A/1:user/release-keys",
      }),
    ).toBe("R5CT60ZQR4M|google/panther/panther:16/BP1A/1:user/release-keys");
  });

  it("falls back to the serial alone when no fingerprint is verified", () => {
    for (const build_fingerprint of [null, "", "   "]) {
      expect(deviceIdentityKey({ serial: "abc", build_fingerprint })).toBe(
        "abc",
      );
    }
  });

  it("separates duplicate serials that differ by build", () => {
    const first = deviceIdentityKey({
      serial: "SHARED",
      build_fingerprint: "brand/a:16/A/1:user/release-keys",
    });
    const second = deviceIdentityKey({
      serial: "SHARED",
      build_fingerprint: "brand/b:16/B/2:user/release-keys",
    });
    expect(first).not.toBe(second);
  });
});
