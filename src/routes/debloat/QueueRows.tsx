import { useTranslation } from "react-i18next";

import { Badge, TableCell, TableHeaderCell } from "../common";
import type { PackageSnapshot, QueueStatus } from "../debloatQueue";
import { QuirkHint } from "./QuirkHint";
import type { DebloatQueueRow, QuirkDeviceContext } from "./queue";

export function QueueRows({
  rows,
  deviceContext,
}: {
  rows: DebloatQueueRow[];
  deviceContext?: QuirkDeviceContext;
}) {
  const { t } = useTranslation();

  return (
    <div className="mt-4 overflow-x-auto rounded-lg border border-white/10">
      <table className="min-w-full text-sm">
        <thead className="bg-white/[0.04]">
          <tr>
            <TableHeaderCell>{t("apps.package")}</TableHeaderCell>
            <TableHeaderCell>{t("devices.state")}</TableHeaderCell>
            <TableHeaderCell>{t("debloat.beforeAfter")}</TableHeaderCell>
            <TableHeaderCell>{t("debloat.journalId")}</TableHeaderCell>
          </tr>
        </thead>
        <tbody className="divide-y divide-white/10">
          {rows.map((row) => (
            <tr key={row.entry.id} className="bg-anvil-950/20">
              <TableCell>
                <code className="font-mono text-xs text-anvil-100">
                  {row.entry.id}
                </code>
                {row.error && (
                  <p className="mt-1 max-w-xl text-xs leading-5 text-red-200/80">
                    {row.error}
                  </p>
                )}
                {deviceContext && row.status === "failed" && row.error && (
                  <QuirkHint
                    packageId={row.entry.id}
                    rawError={row.error}
                    deviceContext={deviceContext}
                  />
                )}
              </TableCell>
              <TableCell>
                <Badge tone={queueStatusTone(row.status)}>
                  {t(`debloat.queueStatus.${row.status}`)}
                </Badge>
                {row.attempts > 0 && (
                  <p className="mt-1 text-[11px] text-anvil-500">
                    {t("debloat.attemptCount", { count: row.attempts })}
                  </p>
                )}
              </TableCell>
              <TableCell>
                <div className="min-w-[12rem] text-xs text-anvil-400">
                  <p>
                    {t("debloat.beforeState", snapshotLabel(row.before, t))}
                  </p>
                  <p className="mt-1">
                    {t("debloat.afterState", snapshotLabel(row.after, t))}
                  </p>
                </div>
              </TableCell>
              <TableCell>
                {row.journalId ? (
                  <code className="font-mono text-xs text-circuit-100">
                    #{row.journalId}
                  </code>
                ) : (
                  <span className="text-xs text-anvil-500">
                    {t("debloat.noJournalEntry")}
                  </span>
                )}
              </TableCell>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function queueStatusTone(
  status: QueueStatus,
): "neutral" | "info" | "success" | "warning" | "danger" {
  switch (status) {
    case "pending":
      return "neutral";
    case "running":
      return "info";
    case "verified":
      return "success";
    case "failed":
      return "danger";
    case "cancelled":
      return "warning";
  }
}

function snapshotLabel(
  snapshot: PackageSnapshot | null,
  t: ReturnType<typeof useTranslation>["t"],
): { state: string } {
  if (!snapshot) return { state: t("debloat.stateUnknown") };
  if (!snapshot.present) return { state: t("debloat.stateMissing") };
  return {
    state: snapshot.enabled
      ? t("debloat.stateEnabled")
      : t("debloat.stateDisabled"),
  };
}
