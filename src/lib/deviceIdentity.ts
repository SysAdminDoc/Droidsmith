import type { DeviceTarget } from "./bindings";

/**
 * Canonical persistence identity for one device.
 *
 * Mirrors `src-tauri/src/device_identity.rs`. Everything Droidsmith persists
 * per device used to be keyed on the ADB serial alone, but serials are not
 * unique: clone and OEM firmware ships duplicated values and some devices
 * report an empty one, so two devices shared one store. Mixing the verified
 * build fingerprint in separates them.
 *
 * The two implementations are pinned to the same literal by
 * `deviceIdentity.test.ts` and `device_identity.rs`'s
 * `canonical_form_is_pinned_and_mirrored_by_the_renderer_helper`. Changing the
 * format on one side without the other silently splits every device's store.
 */
const IDENTITY_SEPARATOR = "|";

/**
 * A device with no verified fingerprint yields the serial alone — byte for
 * byte what pre-fingerprint builds stored, so an existing scope stays
 * addressable rather than being orphaned.
 */
export function deviceIdentityKey(
  target: Pick<DeviceTarget, "serial" | "build_fingerprint">,
): string {
  const fingerprint = target.build_fingerprint?.trim();
  if (!fingerprint) return target.serial;
  return `${target.serial}${IDENTITY_SEPARATOR}${fingerprint}`;
}
