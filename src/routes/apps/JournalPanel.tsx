import { useTranslation } from "react-i18next";

import { type ActionKind, type JournalEntry } from "../../lib/tauri";
import { formatDateTime } from "../../lib/i18n";
import { journalEntryStatus, type JournalEntryStatus } from "../appsJournal";
import {
  Badge,
  Button,
  Card,
  EmptyState,
  SkeletonLine,
  TableCell,
  TableHeaderCell,
} from "../common";
import type { JournalState } from "./types";

/** Reversible-action audit journal with per-entry and per-batch undo (IMP-67:
 *  extracted verbatim from the former Apps.tsx god-file). */
export function JournalPanel({
  state,
  undoingEntryId,
  undoingBatchId,
  onRefresh,
  onUndo,
  onUndoBatch,
}: {
  state: JournalState;
  undoingEntryId: number | null;
  undoingBatchId: string | null;
  onRefresh: () => void;
  onUndo: (entry: JournalEntry) => void;
  onUndoBatch: (batchId: string) => void;
}) {
  const { t, i18n } = useTranslation();

  if (state.kind === "idle") return null;

  const entries =
    state.kind === "ok"
      ? [...state.entries].sort((a, b) => b.id - a.id).slice(0, 16)
      : [];

  return (
    <Card className="overflow-hidden p-0">
      <div className="flex flex-col gap-3 border-b border-white/10 p-4 sm:flex-row sm:items-start sm:justify-between">
        <div>
          <h3 className="text-sm font-semibold text-anvil-50">
            {t("apps.journalTitle")}
          </h3>
          <p className="mt-1 text-xs leading-5 text-anvil-400">
            {t("apps.journalBody")}
          </p>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          {state.kind === "ok" && (
            <Badge tone="neutral">
              {t("apps.journalEntryCount", { count: state.entries.length })}
            </Badge>
          )}
          <Button
            type="button"
            size="sm"
            variant="ghost"
            onClick={onRefresh}
            disabled={state.kind === "loading"}
          >
            {state.kind === "loading"
              ? t("apps.journalLoading")
              : t("apps.journalRefresh")}
          </Button>
        </div>
      </div>

      {state.kind === "loading" && (
        <div className="divide-y divide-white/10">
          {[0, 1, 2].map((i) => (
            <div
              key={i}
              className="grid gap-4 p-4 md:grid-cols-[0.5fr_1.2fr_1.6fr_1fr_1fr]"
            >
              <SkeletonLine className="w-14" />
              <SkeletonLine className="w-32" />
              <SkeletonLine className="w-56" />
              <SkeletonLine className="w-24" />
              <SkeletonLine className="w-20" />
            </div>
          ))}
        </div>
      )}

      {state.kind === "error" && (
        <div className="border-t border-red-300/20 bg-red-950/15 p-4">
          <Badge tone="danger">{t("apps.journalLoadFailed")}</Badge>
          <p className="mt-3 text-sm leading-6 text-red-100">{state.message}</p>
        </div>
      )}

      {state.kind === "ok" && entries.length === 0 && (
        <EmptyState title={t("apps.journalEmpty")}>
          <p>{t("apps.journalEmptyBody")}</p>
        </EmptyState>
      )}

      {state.kind === "ok" && entries.length > 0 && (
        <div className="overflow-x-auto">
          <table className="min-w-full text-sm">
            <thead className="bg-white/[0.04]">
              <tr>
                <TableHeaderCell>{t("apps.journalId")}</TableHeaderCell>
                <TableHeaderCell>{t("apps.journalAction")}</TableHeaderCell>
                <TableHeaderCell>{t("apps.package")}</TableHeaderCell>
                <TableHeaderCell>{t("devices.state")}</TableHeaderCell>
                <TableHeaderCell>{t("apps.journalOutput")}</TableHeaderCell>
                <TableHeaderCell>{t("apps.actions")}</TableHeaderCell>
              </tr>
            </thead>
            <tbody className="divide-y divide-white/10">
              {entries.map((entry) => {
                const status = journalEntryStatus(
                  entry,
                  state.entries.find(
                    (candidate) => candidate.id === entry.undone_by,
                  )?.outcome,
                );
                const request = entry.applied.plan.request;
                const batchId = request.context?.batch_id ?? null;
                const batchUndoableEntries = batchId
                  ? state.entries.filter(
                      (candidate) =>
                        candidate.undoes === null &&
                        candidate.applied.plan.request.context?.batch_id ===
                          batchId &&
                        journalEntryStatus(
                          candidate,
                          state.entries.find(
                            (undo) => undo.id === candidate.undone_by,
                          )?.outcome,
                        ) === "undoable",
                    )
                  : [];
                const batchUndoAnchor = Math.max(
                  ...batchUndoableEntries.map((candidate) => candidate.id),
                );
                return (
                  <tr
                    key={entry.id}
                    className="bg-anvil-950/20 transition hover:bg-white/[0.035]"
                  >
                    <TableCell>
                      <div className="min-w-[6rem]">
                        <code className="font-mono text-xs text-anvil-50">
                          #{entry.id}
                        </code>
                        <p className="mt-1 text-[11px] text-anvil-500">
                          {formatDateTime(
                            entry.applied.applied_at,
                            i18n.resolvedLanguage ?? i18n.language,
                          )}
                        </p>
                      </div>
                    </TableCell>
                    <TableCell>
                      <div className="min-w-[8rem]">
                        <Badge tone="info">
                          {t(journalActionKey(request.kind))}
                        </Badge>
                        <p className="mt-2 max-w-xs text-xs leading-5 text-anvil-400">
                          {entry.applied.plan.description}
                        </p>
                        <p className="mt-1 font-mono text-[10px] text-anvil-500">
                          {t("apps.journalAuditMeta", {
                            incident:
                              entry.applied.plan.incident_id || "legacy",
                            source:
                              request.context?.confirmation_source ?? "legacy",
                          })}
                        </p>
                        {batchId && (
                          <p className="mt-1 font-mono text-[10px] text-circuit-200">
                            {t("apps.journalBatchMeta", {
                              id: batchId,
                              count: state.entries.filter(
                                (candidate) =>
                                  candidate.undoes === null &&
                                  candidate.applied.plan.request.context
                                    ?.batch_id === batchId,
                              ).length,
                            })}
                          </p>
                        )}
                        {(entry.applied.before_state ||
                          entry.applied.after_state) && (
                          <p className="mt-1 text-[10px] text-anvil-500">
                            {t("apps.journalStateChange", {
                              before:
                                entry.applied.before_state ||
                                t("debloat.stateUnknown"),
                              after:
                                entry.applied.after_state ||
                                t("debloat.stateUnknown"),
                            })}
                          </p>
                        )}
                      </div>
                    </TableCell>
                    <TableCell>
                      <code className="block min-w-[16rem] font-mono text-xs text-anvil-100">
                        {request.package || entry.applied.plan.description}
                      </code>
                    </TableCell>
                    <TableCell>
                      <Badge tone={journalStatusTone(status)}>
                        {journalStatusLabel(entry, status, t)}
                      </Badge>
                    </TableCell>
                    <TableCell>
                      <pre className="max-w-[22rem] whitespace-pre-wrap break-words font-mono text-[11px] leading-5 text-anvil-400">
                        {entry.failure ||
                          summarizeJournalOutput(entry.applied.stdout) ||
                          t("apps.journalNoOutput")}
                      </pre>
                    </TableCell>
                    <TableCell>
                      {batchId && batchUndoAnchor === entry.id ? (
                        <Button
                          type="button"
                          size="sm"
                          variant="primary"
                          onClick={() => onUndoBatch(batchId)}
                          disabled={undoingBatchId === batchId}
                        >
                          {undoingBatchId === batchId
                            ? t("apps.journalUndoingBatch")
                            : t("apps.journalUndoBatch", {
                                count: batchUndoableEntries.length,
                              })}
                        </Button>
                      ) : batchId ? (
                        <span className="text-xs text-anvil-500">
                          {t("apps.journalBatchMember")}
                        </span>
                      ) : status === "undoable" ? (
                        <Button
                          type="button"
                          size="sm"
                          variant="primary"
                          onClick={() => onUndo(entry)}
                          disabled={undoingEntryId === entry.id}
                        >
                          {undoingEntryId === entry.id
                            ? t("apps.journalUndoing")
                            : t("apps.journalUndo")}
                        </Button>
                      ) : (
                        <span className="text-xs text-anvil-500">
                          {t("apps.journalNoUndo")}
                        </span>
                      )}
                    </TableCell>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}
    </Card>
  );
}

function journalActionKey(kind: ActionKind): string {
  return `apps.actionKind.${kind}`;
}

function journalStatusTone(
  status: JournalEntryStatus,
): "neutral" | "info" | "success" | "warning" | "danger" {
  switch (status) {
    case "pending":
      return "info";
    case "failed":
      return "danger";
    case "interrupted":
      return "warning";
    case "undo_interrupted":
      return "warning";
    case "undoable":
      return "warning";
    case "undone":
      return "success";
    case "undo_record":
      return "info";
    case "irreversible":
      return "neutral";
  }
}

function journalStatusLabel(
  entry: JournalEntry,
  status: JournalEntryStatus,
  t: ReturnType<typeof useTranslation>["t"],
): string {
  switch (status) {
    case "pending":
      return t("apps.journalPending");
    case "failed":
      return t("apps.journalFailed");
    case "interrupted":
      return t("apps.journalInterrupted");
    case "undo_interrupted":
      return t("apps.journalUndoInterrupted", { id: entry.undone_by });
    case "undoable":
      return t("apps.journalUndoable");
    case "undone":
      return t("apps.journalUndoneBy", { id: entry.undone_by });
    case "undo_record":
      return t("apps.journalUndoRecord", { id: entry.undoes });
    case "irreversible":
      return t("apps.journalIrreversible");
  }
}

function summarizeJournalOutput(output: string): string {
  const trimmed = output.trim();
  if (trimmed.length <= 180) return trimmed;
  return `${trimmed.slice(0, 180)}...`;
}
