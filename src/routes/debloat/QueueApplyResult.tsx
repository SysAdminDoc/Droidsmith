import { useTranslation } from "react-i18next";

import type { Pack } from "../../lib/tauri";
import { Button, Card, StatePanel } from "../common";
import { queueStats } from "../debloatQueue";
import { QueueRows } from "./QueueRows";
import type { DebloatQueueRow, QuirkDeviceContext } from "./queue";

export function QueueApplyResult({
  pack,
  queue,
  cancelled,
  deviceContext,
  onRetryFailed,
  onReset,
}: {
  pack: Pack;
  queue: DebloatQueueRow[];
  cancelled: boolean;
  deviceContext: QuirkDeviceContext;
  onRetryFailed: () => void;
  onReset: () => void;
}) {
  const { t } = useTranslation();
  const stats = queueStats(queue);
  const failedRows = queue.filter((row) => row.status === "failed");
  const tone =
    stats.failed > 0 || cancelled
      ? "warning"
      : stats.verified === stats.total
        ? "success"
        : "info";

  return (
    <>
      <StatePanel
        title={t("debloat.completeTitle", { name: pack.name })}
        tone={tone}
        actions={
          <>
            {failedRows.length > 0 && (
              <Button
                type="button"
                size="sm"
                variant="primary"
                onClick={onRetryFailed}
              >
                {t("debloat.retryFailed", { count: failedRows.length })}
              </Button>
            )}
            <Button type="button" size="sm" onClick={onReset}>
              {t("debloat.startOver")}
            </Button>
          </>
        }
      >
        <p>
          {t("debloat.disabledCount", { count: stats.verified })}
          {stats.failed > 0 &&
            ` ${t("debloat.failedCount", { count: stats.failed })}`}
          {stats.cancelled > 0 &&
            ` ${t("debloat.cancelledCount", { count: stats.cancelled })}`}
        </p>
        {cancelled && <p className="mt-2">{t("debloat.cancelledSummary")}</p>}
        <p className="mt-2">{t("debloat.journalUndo")}</p>
      </StatePanel>

      <Card className="p-4">
        <h4 className="text-xs font-semibold text-anvil-200">
          {t("debloat.queueResults")}
        </h4>
        <QueueRows rows={queue} deviceContext={deviceContext} />
      </Card>
    </>
  );
}
