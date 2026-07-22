import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import {
  errorMessage,
  callListDeviceSettings,
  callPutDeviceSetting,
  deviceTarget,
  type DeviceSetting,
  type DeviceSettingChange,
  type DeviceTarget,
} from "../lib/tauri";
import {
  resolveAuthorizedTarget,
  sameDeviceTarget,
  useAuthorizedDevices,
  useTransportAuthorization,
} from "../lib/useAuthorizedDevices";

import {
  Badge,
  Button,
  Card,
  FieldInput,
  FieldSelect,
  PaneHeader,
  StatePanel,
  TransportBadge,
  TransportTrustNotice,
} from "./common";

type LogEntry = DeviceSettingChange & { logId: number };

let nextLogId = 1;

export default function DeviceSettingsRoute() {
  const { t } = useTranslation();
  const { devicesState, authorizedDevices } = useAuthorizedDevices();
  const [selectedTarget, setSelectedTarget] = useState<DeviceTarget | null>(
    null,
  );
  const {
    accepted: transportOverrideAccepted,
    setAccepted: setTransportOverrideAccepted,
    authorizedTarget,
  } = useTransportAuthorization(selectedTarget);

  const [settings, setSettings] = useState<DeviceSetting[] | null>(null);
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [busyId, setBusyId] = useState<string | null>(null);
  const [log, setLog] = useState<LogEntry[]>([]);

  useEffect(() => {
    const next = resolveAuthorizedTarget(selectedTarget, authorizedDevices);
    if (sameDeviceTarget(selectedTarget, next)) return;
    setSelectedTarget(next);
    setSettings(null);
    setDrafts({});
    setError(null);
    setLog([]);
  }, [authorizedDevices, selectedTarget]);

  const loadSettings = useCallback(async () => {
    if (!authorizedTarget) return;
    setLoading(true);
    setError(null);
    try {
      const result = await callListDeviceSettings(authorizedTarget);
      setSettings(result);
      setDrafts(
        Object.fromEntries(
          result.map((setting) => [setting.id, setting.value ?? ""]),
        ),
      );
    } catch (loadError) {
      setSettings(null);
      setError(errorMessage(loadError));
    } finally {
      setLoading(false);
    }
  }, [authorizedTarget]);

  useEffect(() => {
    void loadSettings();
  }, [loadSettings]);

  const applyValue = useCallback(
    async (setting: DeviceSetting, value: string) => {
      if (!authorizedTarget) return;
      setBusyId(setting.id);
      setError(null);
      try {
        const change = await callPutDeviceSetting(
          authorizedTarget,
          setting.id,
          value,
        );
        setSettings((current) =>
          current
            ? current.map((item) =>
                item.id === setting.id
                  ? { ...item, value: change.new_value }
                  : item,
              )
            : current,
        );
        setDrafts((current) => ({
          ...current,
          [setting.id]: change.new_value,
        }));
        setLog((current) => [{ ...change, logId: nextLogId++ }, ...current]);
      } catch (applyError) {
        setError(errorMessage(applyError));
      } finally {
        setBusyId(null);
      }
    },
    [authorizedTarget],
  );

  // Undo a logged change by writing its captured previous value back.
  const revert = useCallback(
    async (entry: LogEntry) => {
      if (!authorizedTarget || entry.previous_value == null) return;
      setBusyId(entry.id);
      setError(null);
      try {
        const change = await callPutDeviceSetting(
          authorizedTarget,
          entry.id,
          entry.previous_value,
        );
        setSettings((current) =>
          current
            ? current.map((item) =>
                item.id === entry.id
                  ? { ...item, value: change.new_value }
                  : item,
              )
            : current,
        );
        setDrafts((current) => ({ ...current, [entry.id]: change.new_value }));
        setLog((current) => [{ ...change, logId: nextLogId++ }, ...current]);
      } catch (revertError) {
        setError(errorMessage(revertError));
      } finally {
        setBusyId(null);
      }
    },
    [authorizedTarget],
  );

  return (
    <div>
      <PaneHeader
        title={t("tuning.title")}
        milestone="R-082"
        description={t("tuning.description")}
        meta={
          <div className="flex flex-wrap items-center gap-2">
            {selectedTarget && (
              <>
                <Badge tone="info">
                  <code className="font-mono">{selectedTarget.serial}</code>
                </Badge>
                <TransportBadge kind={selectedTarget.transport_kind} />
              </>
            )}
          </div>
        }
        actions={
          settings ? (
            <Button
              type="button"
              variant="ghost"
              size="sm"
              onClick={() => void loadSettings()}
              disabled={loading}
            >
              {t("tuning.refresh")}
            </Button>
          ) : undefined
        }
      />

      <section className="mt-4 max-w-none space-y-3">
        {devicesState.kind === "no_tauri" && (
          <StatePanel title={t("common.desktopRequired")} tone="info">
            <p>{t("tuning.desktopRequiredBody")}</p>
          </StatePanel>
        )}

        {devicesState.kind === "ok" && authorizedDevices.length === 0 && (
          <StatePanel title={t("common.noAuthorized")} tone="warning">
            <p>{t("tuning.noAuthorizedBody")}</p>
          </StatePanel>
        )}

        {authorizedDevices.length > 1 && (
          <Card className="p-4">
            <h3 className="text-sm font-semibold text-anvil-50">
              {t("common.targetDevice")}
            </h3>
            <div className="mt-3 flex flex-wrap gap-2">
              {authorizedDevices.map((d) => (
                <Button
                  key={`${d.transport_id ?? d.serial}:${d.connection_generation}`}
                  type="button"
                  variant={
                    d.transport_id === selectedTarget?.transport_id &&
                    d.connection_generation ===
                      selectedTarget?.connection_generation
                      ? "primary"
                      : "secondary"
                  }
                  size="sm"
                  onClick={() => setSelectedTarget(deviceTarget(d))}
                >
                  {d.model ?? d.serial}
                </Button>
              ))}
            </div>
          </Card>
        )}

        <TransportTrustNotice
          target={selectedTarget}
          accepted={transportOverrideAccepted}
          onAcceptedChange={setTransportOverrideAccepted}
        />

        {error && (
          <StatePanel title={t("tuning.errorTitle")} tone="danger">
            <p className="font-mono text-xs">{error}</p>
          </StatePanel>
        )}

        {selectedTarget && loading && !settings && (
          <Card>
            <p className="text-sm text-anvil-400">{t("tuning.loading")}</p>
          </Card>
        )}

        {selectedTarget && settings && (
          <div className="divide-y divide-white/10 border-y border-white/10">
            {settings.map((setting) => (
              <SettingCard
                key={setting.id}
                setting={setting}
                draft={drafts[setting.id] ?? ""}
                busy={busyId === setting.id}
                disabled={!authorizedTarget}
                onDraftChange={(value) =>
                  setDrafts((current) => ({ ...current, [setting.id]: value }))
                }
                onApply={(value) => void applyValue(setting, value)}
              />
            ))}
          </div>
        )}

        {log.length > 0 && (
          <Card>
            <h3 className="text-sm font-semibold text-anvil-50">
              {t("tuning.changeLog")}
            </h3>
            <ul className="mt-3 space-y-2">
              {log.map((entry) => (
                <li
                  key={entry.logId}
                  className="flex flex-wrap items-center justify-between gap-3 text-xs"
                >
                  <code className="min-w-0 flex-1 truncate font-mono text-anvil-300">
                    {entry.command}
                  </code>
                  <span className="text-anvil-500">
                    {entry.previous_value == null
                      ? t("tuning.previousUnset")
                      : t("tuning.previousValue", {
                          value: entry.previous_value,
                        })}
                  </span>
                  <Button
                    type="button"
                    size="sm"
                    variant="ghost"
                    disabled={
                      entry.previous_value == null ||
                      busyId === entry.id ||
                      !authorizedTarget
                    }
                    onClick={() => void revert(entry)}
                  >
                    {t("tuning.revert")}
                  </Button>
                </li>
              ))}
            </ul>
          </Card>
        )}
      </section>
    </div>
  );
}

function SettingCard({
  setting,
  draft,
  busy,
  disabled,
  onDraftChange,
  onApply,
}: {
  setting: DeviceSetting;
  draft: string;
  busy: boolean;
  disabled: boolean;
  onDraftChange: (value: string) => void;
  onApply: (value: string) => void;
}) {
  const { t } = useTranslation();
  const control = setting.control;
  const trimmed = draft.trim();
  const currentValue = setting.value;
  const changed = trimmed.length > 0 && trimmed !== (currentValue ?? "");
  const preview = `adb shell settings put ${setting.namespace} ${setting.key} ${
    trimmed || "…"
  }`;

  const validationError = validateDraft(control, trimmed, t);

  return (
    <Card className="border-t-0 px-1 py-4">
      <div className="grid items-center gap-4 md:grid-cols-[minmax(13rem,0.9fr)_minmax(9rem,0.45fr)_minmax(15rem,1fr)]">
        <div className="min-w-0">
          <h3 className="text-sm font-semibold text-anvil-50">
            {t(`tuning.items.${setting.id}.label`)}
          </h3>
          <p className="mt-0.5 text-xs text-anvil-400">
            {t(`tuning.items.${setting.id}.help`)}
          </p>
          <Badge tone="neutral" className="mt-1">
            {setting.namespace}
          </Badge>
        </div>

        <dl className="text-sm">
          <div>
            <dt className="text-xs text-anvil-500">{t("tuning.current")}</dt>
            <dd className="mt-0.5 font-mono text-anvil-200">
              {currentValue ?? t("tuning.unset")}
            </dd>
          </div>
        </dl>

        <div className="flex flex-wrap items-center gap-2 md:justify-end">
          {control.kind === "choice" ? (
            <FieldSelect
              aria-label={t(`tuning.items.${setting.id}.label`)}
              value={trimmed}
              disabled={disabled || busy}
              onChange={(event) => onDraftChange(event.target.value)}
            >
              <option value="" disabled>
                {t("tuning.choose")}
              </option>
              {control.options.map((option) => (
                <option key={option.value} value={option.value}>
                  {t(`tuning.options.${setting.id}.${option.value}`, {
                    defaultValue: option.label,
                  })}
                </option>
              ))}
            </FieldSelect>
          ) : (
            <FieldInput
              type="number"
              inputMode="decimal"
              aria-label={t(`tuning.items.${setting.id}.label`)}
              className="w-40"
              value={draft}
              min={control.min}
              max={control.max}
              step={control.kind === "float" ? "any" : 1}
              disabled={disabled || busy}
              onChange={(event) => onDraftChange(event.target.value)}
            />
          )}
          <Button
            type="button"
            size="sm"
            variant="primary"
            disabled={disabled || busy || !changed || validationError != null}
            onClick={() => onApply(trimmed)}
          >
            {busy ? t("tuning.applying") : t("tuning.apply")}
          </Button>
        </div>
      </div>

      {validationError && (
        <p className="mt-2 text-xs text-red-300/80" role="alert">
          {validationError}
        </p>
      )}

      <p className="mt-2 break-all font-mono text-xs text-anvil-500 md:text-end">
        {preview}
      </p>
    </Card>
  );
}

function validateDraft(
  control: DeviceSetting["control"],
  value: string,
  t: ReturnType<typeof useTranslation>["t"],
): string | null {
  if (value.length === 0) return null;
  if (control.kind === "choice") {
    return control.options.some((option) => option.value === value)
      ? null
      : t("tuning.invalidChoice");
  }
  const parsed = Number(value);
  if (!Number.isFinite(parsed)) return t("tuning.notANumber");
  if (parsed < control.min || parsed > control.max) {
    return t("tuning.outOfRange", { min: control.min, max: control.max });
  }
  return null;
}
