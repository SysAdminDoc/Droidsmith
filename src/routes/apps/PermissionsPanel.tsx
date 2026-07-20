import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import {
  errorMessage,
  callListPermissions,
  callSetPermission,
  type DeviceTarget,
  type PermissionInfo,
} from "../../lib/tauri";
import { Button, Card, EmptyState, SkeletonLine } from "../common";

/** Runtime-permission inspector/toggler for a single package (IMP-67:
 *  extracted verbatim from the former Apps.tsx god-file). */
export function PermissionsPanel({
  target,
  pkg,
  userId,
  onClose,
}: {
  target: DeviceTarget;
  pkg: string;
  userId: number;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const [perms, setPerms] = useState<PermissionInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [toggling, setToggling] = useState<string | null>(null);
  const [permError, setPermError] = useState<string | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    setPermError(null);
    try {
      const result = await callListPermissions(target, pkg);
      setPerms(result);
    } catch (e) {
      setPerms([]);
      setPermError(errorMessage(e));
    } finally {
      setLoading(false);
    }
  }, [target, pkg]);

  useEffect(() => {
    void load();
  }, [load]);

  const togglePerm = useCallback(
    async (permission: string, grant: boolean) => {
      setToggling(permission);
      setPermError(null);
      try {
        await callSetPermission(target, pkg, permission, grant, userId);
        setPerms((prev) =>
          prev.map((p) =>
            p.permission === permission ? { ...p, granted: grant } : p,
          ),
        );
      } catch (e) {
        // Many permissions (signature/system, or fixed by policy) can't be
        // changed via `pm grant`. Surface why instead of silently snapping
        // the toggle back, then reload to show the real state.
        setPermError(
          t("apps.permissionToggleFailed", {
            message: errorMessage(e),
          }),
        );
        void load();
      } finally {
        setToggling(null);
      }
    },
    [target, pkg, userId, load, t],
  );

  return (
    <Card className="overflow-hidden p-0">
      <div className="flex items-center justify-between border-b border-white/10 p-4">
        <div>
          <h3 className="text-sm font-semibold text-anvil-50">
            {t("apps.permissions")}
          </h3>
          <code className="mt-1 block font-mono text-xs text-anvil-400">
            {pkg}
          </code>
        </div>
        <Button type="button" size="sm" variant="ghost" onClick={onClose}>
          {t("common.close")}
        </Button>
      </div>
      {permError && (
        <div
          role="alert"
          className="border-b border-red-500/20 bg-red-500/10 px-4 py-3 text-xs text-red-200"
        >
          {permError}
        </div>
      )}
      {loading ? (
        <div className="p-4">
          <SkeletonLine className="w-48" />
          <SkeletonLine className="mt-3 w-64" />
          <SkeletonLine className="mt-3 w-56" />
        </div>
      ) : perms.length === 0 ? (
        <EmptyState title={t("apps.noPermissionsFound")}>
          <p>{t("apps.noPermissionsFoundBody")}</p>
        </EmptyState>
      ) : (
        <div className="max-h-80 divide-y divide-white/10 overflow-y-auto">
          {perms.map((p) => (
            <div
              key={p.permission}
              className="flex items-center justify-between gap-3 px-4 py-2.5"
            >
              <code className="min-w-0 truncate font-mono text-xs text-anvil-200">
                {p.permission}
              </code>
              <Button
                type="button"
                size="sm"
                variant={p.granted ? "secondary" : "danger"}
                onClick={() => void togglePerm(p.permission, !p.granted)}
                disabled={toggling === p.permission}
                aria-pressed={p.granted}
                aria-label={t(
                  p.granted ? "apps.revokePermission" : "apps.grantPermission",
                  { permission: p.permission },
                )}
              >
                {p.granted ? t("apps.granted") : t("apps.denied")}
              </Button>
            </div>
          ))}
        </div>
      )}
    </Card>
  );
}
