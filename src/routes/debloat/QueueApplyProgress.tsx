import { useTranslation } from "react-i18next";

import type { Pack } from "../../lib/tauri";
import { Button, Card } from "../common";
import { queueStats } from "../debloatQueue";
import { QueueRows } from "./QueueRows";
import type { DebloatQueueRow } from "./queue";

export function QueueApplyProgress({
  pack,
  queue,
  currentPackage,
  cancelRequested,
  onCancel,
}: {
  pack: Pack;
  queue: DebloatQueueRow[];
  currentPackage: string | null;
  cancelRequested: boolean;
  onCancel: () => void;
}) {
  const { t } = useTranslation();
  const stats = queueStats(queue);
  const pct =
    stats.total > 0 ? Math.round((stats.completed / stats.total) * 100) : 0;

  return (
    <Card className="p-5">
      <div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
        <div>
          <h3 className="text-sm font-semibold text-anvil-50">
            {t("debloat.applyingPack", { name: pack.name })}
          </h3>
          <p className="mt-1 text-xs text-anvil-400">
            {currentPackage
              ? t("debloat.currentPackage", { package: currentPackage })
              : t("debloat.preparingQueue")}
          </p>
        </div>
        <Button
          type="button"
          size="sm"
          variant="danger"
          onClick={onCancel}
          disabled={cancelRequested}
        >
          {cancelRequested
            ? t("debloat.cancelRequested")
            : t("debloat.cancelAfterCurrent")}
        </Button>
      </div>

      <div className="mt-4">
        <div className="flex items-center justify-between text-xs text-anvil-400">
          <span>
            {t("debloat.progressCount", {
              applied: stats.completed,
              total: stats.total,
            })}
          </span>
          <span>{pct}%</span>
        </div>
        <progress
          aria-hidden="true"
          className="queue-progress mt-2 block h-2 w-full overflow-hidden rounded-sm"
          max={100}
          value={pct}
        />
      </div>

      <QueueRows rows={queue} />
    </Card>
  );
}
