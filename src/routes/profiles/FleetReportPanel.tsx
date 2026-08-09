import { useTranslation } from "react-i18next";

import type {
  ActionStatus,
  FleetReportActionView,
  FleetReportDeviceView,
  FleetReportView,
  Profile,
} from "../../lib/tauri";
import {
  Badge,
  Button,
  Card,
  EmptyState,
  SkeletonLine,
  StatePanel,
  TableCell,
  TableHeaderCell,
  TransportBadge,
} from "../common";

/** Read-only render of a saved fleet run report.
 *
 *  Deliberately offline: opening a report reaches no device and no network, so
 *  a batch can be reviewed on a machine with none of its hardware attached.
 *  The backend hands over a view with every serial already replaced by a
 *  digest, so there is no raw serial in this component to accidentally show.
 *
 *  Resuming a report belongs to `droidsmith-cli run --retry-from`; this panel
 *  renders the same document and points at that command rather than
 *  reimplementing its selection rules. */
export type FleetReportState =
  | { kind: "idle" }
  | { kind: "choosing" }
  | { kind: "loading"; path: string }
  | { kind: "ready"; path: string; report: FleetReportView }
  | { kind: "error"; message: string };

export type FleetApplyState =
  | { kind: "idle" }
  | { kind: "choosing"; apply: boolean }
  | {
      kind: "running";
      apply: boolean;
      path: string;
      operationId: string;
      messages: string[];
    }
  | { kind: "error"; message: string };

/** Short, stable prefix of a SHA-256 digest — enough to tell devices apart in
 *  a list without pretending the whole digest is readable. */
function shortDigest(digest: string): string {
  return digest.slice(0, 12);
}

export function FleetApplyWorkspace({
  state,
  profile,
  deviceCount,
  onRun,
  onCancel,
}: {
  state: FleetApplyState;
  profile: Profile;
  deviceCount: number;
  onRun: (apply: boolean) => void;
  onCancel: () => void;
}) {
  const { t } = useTranslation();
  const busy = state.kind === "choosing" || state.kind === "running";
  const invalid = !profile.name || profile.actions.length === 0;
  return (
    <div className="space-y-3">
      <Card className="space-y-4 p-5">
        <div>
          <h3 className="font-semibold text-anvil-50">
            {t("profiles.fleet.title")}
          </h3>
          <p className="mt-1 max-w-3xl text-sm leading-6 text-anvil-400">
            {t("profiles.fleet.description")}
          </p>
        </div>
        <div className="flex flex-wrap gap-2">
          <Badge tone="info">
            {t("profiles.fleet.profile", { name: profile.name || "—" })}
          </Badge>
          <Badge tone="neutral">
            {t("profiles.fleet.devices", { count: deviceCount })}
          </Badge>
          <Badge tone="neutral">
            {t("profiles.fleet.actions", { count: profile.actions.length })}
          </Badge>
        </div>
        <p className="text-xs text-amber-200/85">
          {t("profiles.fleet.skipNotice")}
        </p>
        <div className="flex flex-wrap gap-2">
          <Button
            variant="primary"
            disabled={busy || invalid || deviceCount === 0}
            onClick={() => onRun(false)}
          >
            {state.kind === "choosing" && !state.apply
              ? t("profiles.fleet.choosing")
              : t("profiles.fleet.plan")}
          </Button>
          <Button
            variant="danger"
            disabled={busy || invalid || deviceCount === 0}
            onClick={() => onRun(true)}
          >
            {state.kind === "choosing" && state.apply
              ? t("profiles.fleet.choosing")
              : t("profiles.fleet.apply")}
          </Button>
          {state.kind === "running" && (
            <Button variant="secondary" onClick={onCancel}>
              {t("profiles.fleet.cancel")}
            </Button>
          )}
        </div>
        {state.kind === "running" && (
          <div
            className="rounded border border-white/10 bg-black/10 p-3"
            aria-live="polite"
          >
            <p className="text-xs text-anvil-400">
              {t("profiles.fleet.writing", { path: state.path })}
            </p>
            <ul className="mt-2 max-h-48 space-y-1 overflow-auto font-mono text-xs text-anvil-200">
              {state.messages.map((message, index) => (
                <li key={`${index}:${message}`}>{message}</li>
              ))}
            </ul>
          </div>
        )}
      </Card>
      {state.kind === "error" && (
        <StatePanel title={t("profiles.fleet.failed")} tone="danger">
          <p className="break-all">{state.message}</p>
        </StatePanel>
      )}
    </div>
  );
}

/** A report can name any journaled action kind, including ones the profile
 *  author UI does not offer. Translate the ones that have a label and show the
 *  raw token for the rest rather than inventing a name for it. */
const REPORT_ACTION_LABEL_KEYS: Partial<Record<string, string>> = {
  disable: "profiles.actions.disable",
  enable: "profiles.actions.enable",
  uninstall_for_user: "profiles.actions.uninstallForUser",
  restore_existing_for_user: "profiles.actions.restoreExistingForUser",
  clear_data: "profiles.actions.clearData",
  force_stop: "profiles.actions.forceStop",
};

export function FleetReportWorkspace({
  state,
  openReport,
  dismiss,
}: {
  state: FleetReportState;
  openReport: () => void;
  dismiss: () => void;
}) {
  const { t } = useTranslation();
  return (
    <div className="space-y-3">
      <Card className="space-y-3 p-5">
        <div>
          <h3 className="font-semibold text-anvil-50">
            {t("profiles.report.title")}
          </h3>
          <p className="mt-1 text-xs text-anvil-400">
            {t("profiles.report.description")}
          </p>
        </div>
        <div className="flex flex-wrap gap-2">
          <Button
            type="button"
            variant="primary"
            onClick={openReport}
            disabled={state.kind === "choosing" || state.kind === "loading"}
          >
            {t("profiles.report.open")}
          </Button>
          {state.kind !== "idle" && (
            <Button type="button" onClick={dismiss}>
              {t("profiles.report.dismiss")}
            </Button>
          )}
        </div>
        <p className="text-xs text-anvil-400">{t("profiles.report.offline")}</p>
      </Card>

      {state.kind === "loading" && (
        <Card className="space-y-3 p-5">
          <SkeletonLine className="w-40" />
          <SkeletonLine className="w-full max-w-xl" />
        </Card>
      )}
      {state.kind === "error" && (
        <StatePanel title={t("profiles.report.failed")} tone="danger">
          <p className="break-all">{state.message}</p>
        </StatePanel>
      )}
      {state.kind === "ready" && (
        <FleetReportSummary path={state.path} report={state.report} />
      )}
    </div>
  );
}

function FleetReportSummary({
  path,
  report,
}: {
  path: string;
  report: FleetReportView;
}) {
  const { t } = useTranslation();
  const { totals } = report;
  return (
    <div className="space-y-3">
      <Card className="space-y-3 p-5">
        <div className="flex flex-wrap items-start justify-between gap-3">
          <div>
            <h3 className="font-semibold text-anvil-50">
              {t("profiles.report.summaryTitle", {
                name: report.profile.name,
              })}
            </h3>
            <p className="mt-1 text-xs text-anvil-400">
              {t("profiles.report.summaryBody", {
                date: report.generated_at,
                count: report.profile.action_count,
              })}
            </p>
            <p className="mt-1 break-all text-xs text-anvil-400">{path}</p>
          </div>
          <div className="flex flex-wrap gap-2">
            <Badge tone={report.apply ? "warning" : "info"}>
              {report.apply
                ? t("profiles.report.modeApply")
                : t("profiles.report.modeDryRun")}
            </Badge>
            <Badge tone={report.success ? "success" : "danger"}>
              {report.success
                ? t("profiles.report.outcomeClean")
                : t("profiles.report.outcomeIncomplete")}
            </Badge>
            <Badge tone="neutral">
              {t("profiles.report.schema", { version: report.schema_version })}
            </Badge>
          </div>
        </div>
        <div className="flex flex-wrap gap-2">
          <Badge tone="neutral">
            {t("profiles.report.totalDevices", { count: totals.devices })}
          </Badge>
          <Badge tone="info">
            {t("profiles.report.totalRan", { count: totals.ran })}
          </Badge>
          {totals.errored > 0 && (
            <Badge tone="danger">
              {t("profiles.report.totalErrored", { count: totals.errored })}
            </Badge>
          )}
          {totals.skipped > 0 && (
            <Badge tone="warning">
              {t("profiles.report.totalSkipped", { count: totals.skipped })}
            </Badge>
          )}
          <Badge tone="neutral">
            {t("profiles.report.totalActions", {
              planned: totals.actions_planned,
              applied: totals.actions_applied,
              failed: totals.actions_failed,
              skipped: totals.actions_skipped,
            })}
          </Badge>
        </div>
        <code className="block break-all font-mono text-xs text-anvil-400">
          {t("profiles.report.profileDigest", {
            profile: shortDigest(report.profile.fingerprint_sha256),
            actions: shortDigest(report.profile.action_set_sha256),
          })}
        </code>
        {!report.success && (
          <p className="text-xs text-anvil-300">
            {t("profiles.report.resumeHint")}
          </p>
        )}
      </Card>

      {report.lineage && (
        <Card className="space-y-2 p-5">
          <h4 className="text-sm font-semibold text-anvil-50">
            {t("profiles.report.lineageTitle", {
              generation: report.lineage.retry_generation,
            })}
          </h4>
          <p className="text-xs text-anvil-400">
            {t("profiles.report.lineageBody", {
              date: report.lineage.source_generated_at,
              digest: shortDigest(report.lineage.source_sha256),
              count: report.lineage.retried_devices.length,
            })}
          </p>
          {report.lineage.excluded_devices.length > 0 && (
            <ul className="space-y-1 text-xs text-anvil-300">
              {report.lineage.excluded_devices.map((excluded) => (
                <li key={excluded.device.identity_sha256}>
                  <code className="font-mono">
                    {shortDigest(excluded.device.identity_sha256)}
                  </code>{" "}
                  — {excluded.reason}
                </li>
              ))}
            </ul>
          )}
          {report.lineage.accepted_drift.length > 0 && (
            <ul className="space-y-1 text-xs text-amber-200">
              {report.lineage.accepted_drift.map((drift) => (
                <li key={drift.code}>
                  <code className="font-mono">{drift.code}</code> —{" "}
                  {drift.message}
                </li>
              ))}
            </ul>
          )}
        </Card>
      )}

      {report.devices.length === 0 ? (
        <EmptyState title={t("profiles.report.emptyTitle")}>
          {t("profiles.report.emptyBody")}
        </EmptyState>
      ) : (
        report.devices.map((device) => (
          <FleetReportDeviceCard
            key={device.device.identity_sha256}
            device={device}
            apply={report.apply}
          />
        ))
      )}
    </div>
  );
}

function FleetReportDeviceCard({
  device,
  apply,
}: {
  device: FleetReportDeviceView;
  apply: boolean;
}) {
  const { t } = useTranslation();
  const outcomeTone =
    device.outcome === "error"
      ? "danger"
      : device.outcome === "skipped"
        ? "warning"
        : device.success
          ? "success"
          : "danger";
  return (
    <Card className="overflow-hidden p-0">
      <div className="flex flex-wrap items-start justify-between gap-3 border-b border-white/10 p-5">
        <div>
          <h4 className="font-semibold text-anvil-50">
            <code className="font-mono">
              {shortDigest(device.device.identity_sha256)}
            </code>
          </h4>
          <p className="mt-1 text-xs text-anvil-400">
            {device.device.fingerprint_bound
              ? t("profiles.report.identityBound")
              : t("profiles.report.identitySerialOnly")}
          </p>
          {device.android_user != null && (
            <p className="mt-1 text-xs text-anvil-400">
              {t("profiles.report.androidUser", { user: device.android_user })}
            </p>
          )}
        </div>
        <div className="flex flex-wrap gap-2">
          <Badge tone={outcomeTone}>
            {t(`profiles.report.outcome.${device.outcome}`)}
          </Badge>
          {device.transport_kind && (
            <TransportBadge kind={device.transport_kind} />
          )}
        </div>
      </div>
      {device.failure_reason && (
        <div className="border-b border-white/10 p-5 text-xs text-anvil-200">
          {device.failure_code && (
            <code className="me-2 font-mono text-anvil-400">
              {device.failure_code}
            </code>
          )}
          <span className="break-all">{device.failure_reason}</span>
        </div>
      )}
      {device.actions.length > 0 && (
        <div className="max-h-72 overflow-auto">
          <table className="w-full text-start text-xs">
            <thead className="sticky top-0 bg-anvil-900">
              <tr>
                <TableHeaderCell>
                  {t("profiles.report.columnPackage")}
                </TableHeaderCell>
                <TableHeaderCell>
                  {t("profiles.report.columnAction")}
                </TableHeaderCell>
                <TableHeaderCell>
                  {t("profiles.report.columnBefore")}
                </TableHeaderCell>
                <TableHeaderCell>
                  {t("profiles.report.columnResult")}
                </TableHeaderCell>
              </tr>
            </thead>
            <tbody className="divide-y divide-white/10">
              {device.actions.map((action) => (
                <tr key={action.index}>
                  <TableCell>
                    <code className="font-mono">{action.package}</code>
                  </TableCell>
                  <TableCell>
                    {REPORT_ACTION_LABEL_KEYS[action.action]
                      ? t(REPORT_ACTION_LABEL_KEYS[action.action] as string)
                      : action.action}
                  </TableCell>
                  <TableCell>{action.before_state}</TableCell>
                  <TableCell>
                    <FleetReportActionResult action={action} apply={apply} />
                  </TableCell>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </Card>
  );
}

const ACTION_STATUS_TONE: Record<
  ActionStatus,
  "success" | "danger" | "neutral"
> = {
  applied: "success",
  failed: "danger",
  skipped: "neutral",
};

function FleetReportActionResult({
  action,
  apply,
}: {
  action: FleetReportActionView;
  apply: boolean;
}) {
  const { t } = useTranslation();
  if (!action.status) {
    // A planned action with no result is not a failure to report: in a dry-run
    // nothing was executed, and in an interrupted apply the run stopped before
    // reaching it. Saying which is more useful than a blank cell.
    return (
      <Badge tone="neutral">
        {apply
          ? t("profiles.report.statusNotReached")
          : t("profiles.report.statusPlanned")}
      </Badge>
    );
  }
  return (
    <>
      <Badge tone={ACTION_STATUS_TONE[action.status]}>
        {t(`profiles.report.status.${action.status}`)}
      </Badge>
      {action.error && (
        <p className="mt-1 break-all text-anvil-400">{action.error}</p>
      )}
    </>
  );
}
