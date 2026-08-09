const PROTECTED_DEVICE_PATHS = new Set([
  "/",
  "/sdcard",
  "/storage",
  "/storage/emulated",
  "/data",
  "/system",
  "/system_ext",
  "/product",
  "/vendor",
  "/apex",
]);

export type FileMutationBlockReason = "protected" | "permissions" | null;

/**
 * Return whether a parsed directory mode proves that mutations are
 * impossible. `null` deliberately means unknown: OEM `ls` output that does
 * not match the POSIX shape must not be guessed into a disabled action.
 */
export function directoryMutationAllowed(
  permissions: string | null | undefined,
): boolean | null {
  if (!permissions || !/^d[rwxstST-]{9}$/u.test(permissions)) return null;
  const bits = permissions.slice(1);
  const hasWrite = bits[1] === "w" || bits[4] === "w" || bits[7] === "w";
  const hasExecute = [2, 5, 8].some((index) =>
    /[xstST]/u.test(bits[index] ?? ""),
  );
  return hasWrite && hasExecute;
}

export function isProtectedDevicePath(path: string): boolean {
  return PROTECTED_DEVICE_PATHS.has(path);
}

export function mutationBlockReasonForDirectory(
  directoryPermissions: string | null | undefined,
): FileMutationBlockReason {
  return directoryMutationAllowed(directoryPermissions) === false
    ? "permissions"
    : null;
}

export function mutationBlockReasonForEntry(
  fullPath: string,
  directoryPermissions: string | null | undefined,
): FileMutationBlockReason {
  if (isProtectedDevicePath(fullPath)) return "protected";
  return mutationBlockReasonForDirectory(directoryPermissions);
}
