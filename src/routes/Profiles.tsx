import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import {
  errorMessage,
  callGetDeviceInfo,
  callInspectProfile,
  callListPackages,
  callListUsers,
  callSaveProfile,
  callSelectHostPath,
  deviceTarget,
  type ActionKind,
  type AndroidUser,
  type AppPackage,
  type Device,
  type DeviceInfo,
  type Profile,
  type ProfileAction,
  type ProfilePreview,
  type ProfilePreviewStatus,
  type ProfileUserMode,
} from "../lib/tauri";
import {
  useAuthorizedDevices,
  useTransportAuthorization,
} from "../lib/useAuthorizedDevices";
import {
  Badge,
  Button,
  Card,
  EmptyState,
  FieldInput,
  PaneHeader,
  SkeletonLine,
  StatePanel,
  TableCell,
  TableHeaderCell,
  TransportBadge,
  TransportTrustNotice,
} from "./common";

const PROFILE_ACTIONS: ReadonlyArray<{
  value: Exclude<
    ActionKind,
    "grant_permission" | "revoke_permission" | "shell"
  >;
  labelKey: string;
}> = [
  { value: "disable", labelKey: "profiles.actions.disable" },
  { value: "enable", labelKey: "profiles.actions.enable" },
  {
    value: "uninstall_for_user",
    labelKey: "profiles.actions.uninstallForUser",
  },
  {
    value: "restore_existing_for_user",
    labelKey: "profiles.actions.restoreExistingForUser",
  },
  { value: "clear_data", labelKey: "profiles.actions.clearData" },
  { value: "force_stop", labelKey: "profiles.actions.forceStop" },
];

type InventoryState =
  | { kind: "idle" }
  | { kind: "loading" }
  | {
      kind: "ready";
      users: AndroidUser[];
      packages: AppPackage[];
      info: DeviceInfo;
    }
  | { kind: "error"; message: string };

type Notice = {
  tone: "neutral" | "info" | "success" | "warning" | "danger";
  title: string;
  body: string;
};

type PreviewState =
  | { kind: "idle" }
  | { kind: "choosing" }
  | { kind: "loading"; path: string }
  | { kind: "ready"; path: string; preview: ProfilePreview }
  | { kind: "error"; message: string };

export default function ProfilesRoute() {
  const { t } = useTranslation();
  const { devicesState, authorizedDevices } = useAuthorizedDevices();
  const [selectedTransportId, setSelectedTransportId] = useState<number | null>(
    null,
  );
  const [selectedSerial, setSelectedSerial] = useState<string | null>(null);
  const [workspace, setWorkspace] = useState<"author" | "import">("author");
  const [inventory, setInventory] = useState<InventoryState>({ kind: "idle" });
  const [profileName, setProfileName] = useState("");
  const [description, setDescription] = useState("");
  const [serialPrefix, setSerialPrefix] = useState("");
  const [manufacturer, setManufacturer] = useState("");
  const [model, setModel] = useState("");
  const [androidMin, setAndroidMin] = useState("");
  const [androidMax, setAndroidMax] = useState("");
  const [userMode, setUserMode] = useState<ProfileUserMode>("current");
  const [explicitUser, setExplicitUser] = useState<number | null>(null);
  const [packageSearch, setPackageSearch] = useState("");
  const [selectedPackages, setSelectedPackages] = useState<Set<string>>(
    () => new Set(),
  );
  const [actionKind, setActionKind] =
    useState<ProfileAction["kind"]>("disable");
  const [actions, setActions] = useState<ProfileAction[]>([]);
  const [notice, setNotice] = useState<Notice | null>(null);
  const [previewState, setPreviewState] = useState<PreviewState>({
    kind: "idle",
  });

  const selectedDevice =
    authorizedDevices.find((device) =>
      selectedTransportId != null
        ? device.transport_id === selectedTransportId
        : device.serial === selectedSerial,
    ) ?? null;
  const selectedTarget = useMemo(
    () => (selectedDevice ? deviceTarget(selectedDevice) : null),
    [selectedDevice],
  );
  const {
    accepted: transportOverrideAccepted,
    setAccepted: setTransportOverrideAccepted,
    authorizedTarget,
  } = useTransportAuthorization(selectedTarget);

  useEffect(() => {
    const stillPresent = authorizedDevices.find((device) =>
      selectedTransportId != null
        ? device.transport_id === selectedTransportId
        : device.serial === selectedSerial,
    );
    if (stillPresent) return;
    const next = authorizedDevices.length === 1 ? authorizedDevices[0]! : null;
    setSelectedTransportId(next?.transport_id ?? null);
    setSelectedSerial(next?.serial ?? null);
  }, [authorizedDevices, selectedSerial, selectedTransportId]);

  useEffect(() => {
    let current = true;
    setSelectedPackages(new Set());
    setActions([]);
    setPreviewState({ kind: "idle" });
    setNotice(null);
    if (!authorizedTarget) {
      setInventory({ kind: "idle" });
      return () => {
        current = false;
      };
    }

    setInventory({ kind: "loading" });
    Promise.all([
      callListUsers(authorizedTarget),
      callListPackages(authorizedTarget, "all", 0),
      callGetDeviceInfo(authorizedTarget),
    ])
      .then(([users, packages, info]) => {
        if (!current) return;
        setInventory({ kind: "ready", users, packages, info });
        const foreground = users.find((user) => user.current) ?? users[0];
        setExplicitUser(foreground?.id ?? null);
      })
      .catch((error: unknown) => {
        if (!current) return;
        setInventory({
          kind: "error",
          message: errorMessage(error),
        });
      });
    return () => {
      current = false;
    };
  }, [authorizedTarget]);

  const visiblePackages = useMemo(() => {
    if (inventory.kind !== "ready") return [];
    const query = packageSearch.trim().toLocaleLowerCase();
    const matches = query
      ? inventory.packages.filter((pkg) =>
          pkg.package.toLocaleLowerCase().includes(query),
        )
      : inventory.packages;
    return matches.slice(0, 200);
  }, [inventory, packageSearch]);

  const currentProfile = useMemo<Profile>(
    () => ({
      name: profileName.trim(),
      version: "2",
      description: description.trim(),
      device: {
        require_serial_prefix: serialPrefix.trim(),
        require_manufacturer: manufacturer.trim(),
        require_model: model.trim(),
        require_android_min: parseOptionalInteger(androidMin),
        require_android_max: parseOptionalInteger(androidMax),
      },
      user: {
        mode: userMode,
        id: userMode === "explicit" ? explicitUser : null,
      },
      actions,
    }),
    [
      actions,
      androidMax,
      androidMin,
      description,
      explicitUser,
      manufacturer,
      model,
      profileName,
      serialPrefix,
      userMode,
    ],
  );

  function selectDevice(device: Device) {
    setSelectedTransportId(device.transport_id);
    setSelectedSerial(device.serial);
  }

  function populateDetectedConstraints() {
    if (inventory.kind !== "ready" || !selectedDevice) return;
    setSerialPrefix(selectedDevice.serial);
    setManufacturer(inventory.info.manufacturer ?? "");
    setModel(inventory.info.model ?? selectedDevice.model ?? "");
    const sdk = inventory.info.sdk_level ?? "";
    setAndroidMin(sdk);
    setAndroidMax(sdk);
  }

  function addSelectedActions() {
    if (selectedPackages.size === 0) return;
    const additions = [...selectedPackages]
      .filter(
        (pkg) =>
          !actions.some(
            (action) => action.package === pkg && action.kind === actionKind,
          ),
      )
      .map((pkg) => ({ kind: actionKind, package: pkg }));
    setActions((current) => [...current, ...additions]);
    setSelectedPackages(new Set());
    setNotice(null);
  }

  async function saveProfile(profile: Profile, suggestedName: string) {
    setNotice(null);
    try {
      const grant = await callSelectHostPath("profile_save", suggestedName);
      if (!grant) return;
      const artifact = await callSaveProfile(grant.id, profile);
      setNotice({
        tone: "success",
        title: t("profiles.savedTitle"),
        body: t("profiles.savedBody", { path: artifact.local_path }),
      });
    } catch (error) {
      setNotice({
        tone: "danger",
        title: t("profiles.saveFailed"),
        body: errorMessage(error),
      });
    }
  }

  function exportAuthoredProfile() {
    if (!currentProfile.name) {
      setNotice({
        tone: "warning",
        title: t("profiles.nameRequired"),
        body: t("profiles.nameRequiredBody"),
      });
      return;
    }
    if (currentProfile.actions.length === 0) {
      setNotice({
        tone: "warning",
        title: t("profiles.actionsRequired"),
        body: t("profiles.actionsRequiredBody"),
      });
      return;
    }
    const suggested = `${fileSafeName(currentProfile.name)}.yaml`;
    void saveProfile(currentProfile, suggested);
  }

  async function importProfile() {
    if (!authorizedTarget) return;
    setNotice(null);
    setPreviewState({ kind: "choosing" });
    try {
      const grant = await callSelectHostPath("profile_open");
      if (!grant) {
        setPreviewState({ kind: "idle" });
        return;
      }
      setPreviewState({ kind: "loading", path: grant.local_path });
      const preview = await callInspectProfile(authorizedTarget, grant.id);
      setPreviewState({ kind: "ready", path: grant.local_path, preview });
    } catch (error) {
      setPreviewState({
        kind: "error",
        message: errorMessage(error),
      });
    }
  }

  return (
    <div>
      <PaneHeader
        title={t("profiles.title")}
        milestone="R-034"
        description={t("profiles.description")}
        meta={
          <div className="flex flex-wrap items-center gap-2">
            <Badge tone="info">{t("profiles.schemaBadge")}</Badge>
            {selectedTarget && (
              <>
                <Badge tone="neutral">
                  <code className="font-mono">{selectedTarget.serial}</code>
                </Badge>
                <TransportBadge kind={selectedTarget.transport_kind} />
              </>
            )}
          </div>
        }
      />

      <section className="mt-6 max-w-7xl space-y-4">
        {devicesState.kind === "no_tauri" && (
          <StatePanel title={t("common.desktopRequired")} tone="info">
            <p>{t("profiles.desktopRequiredBody")}</p>
          </StatePanel>
        )}
        {devicesState.kind === "error" && (
          <StatePanel title={t("devices.scanFailed")} tone="danger">
            <p>{devicesState.message}</p>
          </StatePanel>
        )}
        {devicesState.kind === "ok" && authorizedDevices.length === 0 && (
          <StatePanel title={t("common.noAuthorized")} tone="warning">
            <p>{t("profiles.noAuthorizedBody")}</p>
          </StatePanel>
        )}
        {devicesState.kind === "loading" && (
          <Card className="space-y-3 p-5">
            <SkeletonLine className="w-40" />
            <SkeletonLine className="w-full max-w-xl" />
          </Card>
        )}
        {authorizedDevices.length > 1 && (
          <DeviceChoice
            devices={authorizedDevices}
            selectedTransportId={selectedTransportId}
            selectedSerial={selectedSerial}
            onSelect={selectDevice}
          />
        )}
        <TransportTrustNotice
          target={selectedTarget}
          accepted={transportOverrideAccepted}
          onAcceptedChange={setTransportOverrideAccepted}
        />
        {inventory.kind === "error" && (
          <StatePanel title={t("profiles.inventoryFailed")} tone="danger">
            <p>{inventory.message}</p>
          </StatePanel>
        )}
        {notice && (
          <StatePanel title={notice.title} tone={notice.tone}>
            <p className="break-all">{notice.body}</p>
          </StatePanel>
        )}

        {selectedTarget && (
          <>
            <div
              className="inline-flex rounded-lg border border-white/10 bg-anvil-900/70 p-1"
              role="tablist"
              aria-label={t("profiles.workspaceLabel")}
            >
              <WorkspaceTab
                active={workspace === "author"}
                onClick={() => setWorkspace("author")}
              >
                {t("profiles.authorTab")}
              </WorkspaceTab>
              <WorkspaceTab
                active={workspace === "import"}
                onClick={() => setWorkspace("import")}
              >
                {t("profiles.importTab")}
              </WorkspaceTab>
            </div>

            {workspace === "author" ? (
              <AuthorWorkspace
                inventory={inventory}
                profileName={profileName}
                setProfileName={setProfileName}
                description={description}
                setDescription={setDescription}
                serialPrefix={serialPrefix}
                setSerialPrefix={setSerialPrefix}
                manufacturer={manufacturer}
                setManufacturer={setManufacturer}
                model={model}
                setModel={setModel}
                androidMin={androidMin}
                setAndroidMin={setAndroidMin}
                androidMax={androidMax}
                setAndroidMax={setAndroidMax}
                populateDetectedConstraints={populateDetectedConstraints}
                userMode={userMode}
                setUserMode={setUserMode}
                explicitUser={explicitUser}
                setExplicitUser={setExplicitUser}
                packageSearch={packageSearch}
                setPackageSearch={setPackageSearch}
                visiblePackages={visiblePackages}
                selectedPackages={selectedPackages}
                setSelectedPackages={setSelectedPackages}
                actionKind={actionKind}
                setActionKind={setActionKind}
                actions={actions}
                setActions={setActions}
                addSelectedActions={addSelectedActions}
                exportProfile={exportAuthoredProfile}
              />
            ) : (
              <ImportWorkspace
                state={previewState}
                importProfile={() => void importProfile()}
                saveMigration={(profile) =>
                  void saveProfile(
                    profile,
                    `${fileSafeName(profile.name)}-v2.yaml`,
                  )
                }
              />
            )}
          </>
        )}
      </section>
    </div>
  );
}

function AuthorWorkspace(props: {
  inventory: InventoryState;
  profileName: string;
  setProfileName: (value: string) => void;
  description: string;
  setDescription: (value: string) => void;
  serialPrefix: string;
  setSerialPrefix: (value: string) => void;
  manufacturer: string;
  setManufacturer: (value: string) => void;
  model: string;
  setModel: (value: string) => void;
  androidMin: string;
  setAndroidMin: (value: string) => void;
  androidMax: string;
  setAndroidMax: (value: string) => void;
  populateDetectedConstraints: () => void;
  userMode: ProfileUserMode;
  setUserMode: (value: ProfileUserMode) => void;
  explicitUser: number | null;
  setExplicitUser: (value: number | null) => void;
  packageSearch: string;
  setPackageSearch: (value: string) => void;
  visiblePackages: AppPackage[];
  selectedPackages: Set<string>;
  setSelectedPackages: (value: Set<string>) => void;
  actionKind: ProfileAction["kind"];
  setActionKind: (value: ProfileAction["kind"]) => void;
  actions: ProfileAction[];
  setActions: (value: ProfileAction[]) => void;
  addSelectedActions: () => void;
  exportProfile: () => void;
}) {
  const { t } = useTranslation();
  const users = props.inventory.kind === "ready" ? props.inventory.users : [];

  return (
    <div className="grid gap-4 xl:grid-cols-[minmax(0,0.9fr)_minmax(0,1.1fr)]">
      <div className="space-y-4">
        <Card className="p-5">
          <h3 className="text-base font-semibold text-anvil-50">
            {t("profiles.identityTitle")}
          </h3>
          <div className="mt-4 grid gap-4">
            <LabeledField label={t("profiles.nameLabel")}>
              <FieldInput
                value={props.profileName}
                onChange={(event) => props.setProfileName(event.target.value)}
                placeholder={t("profiles.namePlaceholder")}
                maxLength={120}
              />
            </LabeledField>
            <LabeledField label={t("profiles.descriptionLabel")}>
              <textarea
                className="min-h-24 rounded-md border border-white/10 bg-white/[0.06] px-3 py-2 text-sm text-anvil-50 outline-none placeholder:text-anvil-600 focus:border-circuit-300/60 focus:ring-2 focus:ring-circuit-300/20"
                value={props.description}
                onChange={(event) => props.setDescription(event.target.value)}
                placeholder={t("profiles.descriptionPlaceholder")}
                maxLength={1000}
              />
            </LabeledField>
          </div>
        </Card>

        <Card className="p-5">
          <div className="flex flex-wrap items-start justify-between gap-3">
            <div>
              <h3 className="text-base font-semibold text-anvil-50">
                {t("profiles.constraintsTitle")}
              </h3>
              <p className="mt-1 text-sm leading-6 text-anvil-400">
                {t("profiles.constraintsBody")}
              </p>
            </div>
            <Button
              size="sm"
              onClick={props.populateDetectedConstraints}
              disabled={props.inventory.kind !== "ready"}
            >
              {t("profiles.useDetected")}
            </Button>
          </div>
          <div className="mt-4 grid gap-4 sm:grid-cols-2">
            <LabeledField label={t("profiles.serialPrefixLabel")}>
              <FieldInput
                value={props.serialPrefix}
                onChange={(event) => props.setSerialPrefix(event.target.value)}
              />
            </LabeledField>
            <LabeledField label={t("profiles.manufacturerLabel")}>
              <FieldInput
                value={props.manufacturer}
                onChange={(event) => props.setManufacturer(event.target.value)}
              />
            </LabeledField>
            <LabeledField label={t("profiles.modelLabel")}>
              <FieldInput
                value={props.model}
                onChange={(event) => props.setModel(event.target.value)}
              />
            </LabeledField>
            <div className="grid grid-cols-2 gap-3">
              <LabeledField label={t("profiles.androidMinLabel")}>
                <FieldInput
                  type="number"
                  min={1}
                  value={props.androidMin}
                  onChange={(event) => props.setAndroidMin(event.target.value)}
                />
              </LabeledField>
              <LabeledField label={t("profiles.androidMaxLabel")}>
                <FieldInput
                  type="number"
                  min={1}
                  value={props.androidMax}
                  onChange={(event) => props.setAndroidMax(event.target.value)}
                />
              </LabeledField>
            </div>
          </div>
          <div className="mt-4 grid gap-4 sm:grid-cols-2">
            <LabeledField label={t("profiles.userModeLabel")}>
              <SelectField
                value={props.userMode}
                onChange={(value) =>
                  props.setUserMode(value as ProfileUserMode)
                }
              >
                <option value="owner">{t("profiles.userOwner")}</option>
                <option value="current">{t("profiles.userCurrent")}</option>
                <option value="explicit">{t("profiles.userExplicit")}</option>
              </SelectField>
            </LabeledField>
            {props.userMode === "explicit" && (
              <LabeledField label={t("profiles.explicitUserLabel")}>
                <SelectField
                  value={props.explicitUser ?? ""}
                  onChange={(value) =>
                    props.setExplicitUser(value === "" ? null : Number(value))
                  }
                >
                  <option value="">{t("profiles.selectUser")}</option>
                  {users.map((user) => (
                    <option key={user.id} value={user.id}>
                      {user.name} ({user.id})
                    </option>
                  ))}
                </SelectField>
              </LabeledField>
            )}
          </div>
        </Card>
      </div>

      <div className="space-y-4">
        <Card className="p-5">
          <div className="flex flex-wrap items-start justify-between gap-3">
            <div>
              <h3 className="text-base font-semibold text-anvil-50">
                {t("profiles.actionsTitle")}
              </h3>
              <p className="mt-1 text-sm leading-6 text-anvil-400">
                {t("profiles.actionsBody")}
              </p>
            </div>
            <Badge tone="neutral">
              {t("profiles.actionCount", { count: props.actions.length })}
            </Badge>
          </div>
          <div className="mt-4 grid gap-3 sm:grid-cols-[minmax(0,1fr)_minmax(180px,0.55fr)_auto]">
            <FieldInput
              aria-label={t("profiles.packageSearchLabel")}
              placeholder={t("profiles.packageSearchPlaceholder")}
              value={props.packageSearch}
              onChange={(event) => props.setPackageSearch(event.target.value)}
            />
            <SelectField
              ariaLabel={t("profiles.actionKindLabel")}
              value={props.actionKind}
              onChange={(value) =>
                props.setActionKind(value as ProfileAction["kind"])
              }
            >
              {PROFILE_ACTIONS.map((action) => (
                <option key={action.value} value={action.value}>
                  {t(action.labelKey)}
                </option>
              ))}
            </SelectField>
            <Button
              variant="primary"
              onClick={props.addSelectedActions}
              disabled={props.selectedPackages.size === 0}
            >
              {t("profiles.addSelected", {
                count: props.selectedPackages.size,
              })}
            </Button>
          </div>

          {props.inventory.kind === "loading" && (
            <div className="mt-4 space-y-3">
              <SkeletonLine className="w-full" />
              <SkeletonLine className="w-4/5" />
              <SkeletonLine className="w-3/5" />
            </div>
          )}
          {props.inventory.kind === "ready" &&
            props.visiblePackages.length === 0 && (
              <EmptyState title={t("profiles.noPackagesTitle")}>
                <p>{t("profiles.noPackagesBody")}</p>
              </EmptyState>
            )}
          {props.visiblePackages.length > 0 && (
            <div className="mt-4 max-h-72 overflow-auto rounded-md border border-white/10">
              {props.visiblePackages.map((pkg) => (
                <label
                  key={pkg.package}
                  className="flex cursor-pointer items-center gap-3 border-b border-white/10 px-3 py-2.5 text-sm last:border-b-0 hover:bg-white/[0.04]"
                >
                  <input
                    type="checkbox"
                    className="h-4 w-4 accent-circuit-300"
                    checked={props.selectedPackages.has(pkg.package)}
                    onChange={(event) => {
                      const next = new Set(props.selectedPackages);
                      if (event.target.checked) next.add(pkg.package);
                      else next.delete(pkg.package);
                      props.setSelectedPackages(next);
                    }}
                  />
                  <code className="min-w-0 flex-1 break-all font-mono text-xs text-anvil-100">
                    {pkg.package}
                  </code>
                  <Badge tone={pkg.system ? "info" : "neutral"}>
                    {pkg.system
                      ? t("profiles.systemPackage")
                      : t("profiles.userPackage")}
                  </Badge>
                </label>
              ))}
            </div>
          )}
          {props.inventory.kind === "ready" &&
            props.inventory.packages.length > 200 &&
            !props.packageSearch.trim() && (
              <p className="mt-2 text-xs text-anvil-500">
                {t("profiles.packageLimitHint")}
              </p>
            )}
        </Card>

        <Card className="p-5">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <h3 className="text-base font-semibold text-anvil-50">
              {t("profiles.reviewTitle")}
            </h3>
            <Button variant="primary" onClick={props.exportProfile}>
              {t("profiles.exportButton")}
            </Button>
          </div>
          {props.actions.length === 0 ? (
            <EmptyState title={t("profiles.noActionsTitle")}>
              <p>{t("profiles.noActionsBody")}</p>
            </EmptyState>
          ) : (
            <ol className="mt-4 divide-y divide-white/10">
              {props.actions.map((action, index) => (
                <li
                  key={`${action.kind}:${action.package}:${index}`}
                  className="flex items-start gap-3 py-3 first:pt-0 last:pb-0"
                >
                  <span className="grid h-6 w-6 shrink-0 place-items-center rounded border border-white/10 font-mono text-xs text-anvil-400">
                    {index + 1}
                  </span>
                  <div className="min-w-0 flex-1">
                    <Badge tone="info">{action.kind}</Badge>
                    <code className="mt-2 block break-all font-mono text-xs text-anvil-100">
                      {action.package}
                    </code>
                  </div>
                  <Button
                    size="sm"
                    variant="ghost"
                    aria-label={t("profiles.removeAction", {
                      package: action.package,
                    })}
                    onClick={() =>
                      props.setActions(
                        props.actions.filter(
                          (_, actionIndex) => actionIndex !== index,
                        ),
                      )
                    }
                  >
                    {t("profiles.remove")}
                  </Button>
                </li>
              ))}
            </ol>
          )}
        </Card>
      </div>
    </div>
  );
}

function ImportWorkspace({
  state,
  importProfile,
  saveMigration,
}: {
  state: PreviewState;
  importProfile: () => void;
  saveMigration: (profile: Profile) => void;
}) {
  const { t } = useTranslation();
  const busy = state.kind === "choosing" || state.kind === "loading";
  return (
    <div className="space-y-4">
      <Card className="p-5">
        <div className="flex flex-wrap items-start justify-between gap-4">
          <div>
            <h3 className="text-base font-semibold text-anvil-50">
              {t("profiles.importTitle")}
            </h3>
            <p className="mt-1 max-w-3xl text-sm leading-6 text-anvil-400">
              {t("profiles.importBody")}
            </p>
          </div>
          <Button variant="primary" disabled={busy} onClick={importProfile}>
            {busy ? t("profiles.importing") : t("profiles.chooseProfile")}
          </Button>
        </div>
      </Card>
      {state.kind === "loading" && (
        <StatePanel title={t("profiles.previewing")} tone="info">
          <p className="break-all">{state.path}</p>
        </StatePanel>
      )}
      {state.kind === "error" && (
        <StatePanel title={t("profiles.importFailed")} tone="danger">
          <p>{state.message}</p>
        </StatePanel>
      )}
      {state.kind === "ready" && (
        <ProfileDiff
          path={state.path}
          preview={state.preview}
          saveMigration={saveMigration}
        />
      )}
    </div>
  );
}

function ProfileDiff({
  path,
  preview,
  saveMigration,
}: {
  path: string;
  preview: ProfilePreview;
  saveMigration: (profile: Profile) => void;
}) {
  const { t } = useTranslation();
  return (
    <>
      {preview.migration && (
        <StatePanel
          title={t("profiles.migrationTitle", {
            from: preview.migration.from_version,
            to: preview.migration.to_version,
          })}
          tone="warning"
          actions={
            <Button
              variant="primary"
              onClick={() => saveMigration(preview.migration!.profile)}
            >
              {t("profiles.saveMigrated")}
            </Button>
          }
        >
          <p>{t("profiles.migrationBody")}</p>
          {preview.migration.warnings.length > 0 && (
            <ul className="mt-2 list-disc space-y-1 pl-5">
              {preview.migration.warnings.map((warning) => (
                <li key={warning}>{warning}</li>
              ))}
            </ul>
          )}
        </StatePanel>
      )}
      <Card className="p-5">
        <div className="flex flex-wrap items-start justify-between gap-4">
          <div className="min-w-0">
            <div className="flex flex-wrap items-center gap-2">
              <h3 className="text-lg font-semibold text-anvil-50">
                {preview.profile.name}
              </h3>
              <Badge tone={preview.compatible ? "success" : "danger"}>
                {preview.compatible
                  ? t("profiles.compatible")
                  : t("profiles.incompatible")}
              </Badge>
              <Badge tone="neutral">
                {t("profiles.sourceVersion", {
                  version: preview.source_version,
                })}
              </Badge>
            </div>
            <p className="mt-2 break-all font-mono text-xs text-anvil-500">
              {path}
            </p>
            {preview.profile.description && (
              <p className="mt-3 text-sm leading-6 text-anvil-300">
                {preview.profile.description}
              </p>
            )}
          </div>
          <Badge tone="info">
            {preview.android_user == null
              ? t("profiles.unresolvedUser")
              : t("profiles.resolvedUser", { id: preview.android_user })}
          </Badge>
        </div>
        {preview.compatibility_issues.length > 0 && (
          <div className="mt-4 rounded-md border border-red-300/20 bg-red-300/[0.06] p-4">
            <p className="text-sm font-semibold text-red-100">
              {t("profiles.compatibilityIssues")}
            </p>
            <ul className="mt-2 list-disc space-y-1 pl-5 text-sm text-red-100/90">
              {preview.compatibility_issues.map((issue) => (
                <li key={issue}>{issue}</li>
              ))}
            </ul>
          </div>
        )}
      </Card>
      <Card className="overflow-hidden p-0">
        <div className="flex flex-wrap items-center justify-between gap-3 border-b border-white/10 px-5 py-4">
          <div>
            <h3 className="text-base font-semibold text-anvil-50">
              {t("profiles.diffTitle")}
            </h3>
            <p className="mt-1 text-sm text-anvil-400">
              {t("profiles.diffBody")}
            </p>
          </div>
          <Badge tone="neutral">
            {t("profiles.actionCount", { count: preview.rows.length })}
          </Badge>
        </div>
        {preview.rows.length === 0 ? (
          <EmptyState title={t("profiles.noDiffTitle")}>
            <p>{t("profiles.noDiffBody")}</p>
          </EmptyState>
        ) : (
          <div className="overflow-x-auto">
            <table className="min-w-full text-sm">
              <thead className="bg-white/[0.025]">
                <tr>
                  <TableHeaderCell>
                    {t("profiles.packageColumn")}
                  </TableHeaderCell>
                  <TableHeaderCell>
                    {t("profiles.actionColumn")}
                  </TableHeaderCell>
                  <TableHeaderCell>
                    {t("profiles.changeColumn")}
                  </TableHeaderCell>
                  <TableHeaderCell>
                    {t("profiles.statusColumn")}
                  </TableHeaderCell>
                  <TableHeaderCell>
                    {t("profiles.commandColumn")}
                  </TableHeaderCell>
                </tr>
              </thead>
              <tbody className="divide-y divide-white/10">
                {preview.rows.map((row, index) => (
                  <tr key={`${row.action.kind}:${row.action.package}:${index}`}>
                    <TableCell>
                      <code className="break-all font-mono text-xs text-anvil-100">
                        {row.action.package}
                      </code>
                    </TableCell>
                    <TableCell>
                      <Badge tone="info">{row.action.kind}</Badge>
                    </TableCell>
                    <TableCell>
                      <span className="whitespace-nowrap">
                        {row.current_state} &rarr; {row.expected_state}
                      </span>
                    </TableCell>
                    <TableCell>
                      <div className="space-y-1">
                        <Badge tone={previewTone(row.status)}>
                          {t(`profiles.status.${row.status}`)}
                        </Badge>
                        <p className="max-w-xs text-xs leading-5 text-anvil-400">
                          {row.reason}
                        </p>
                      </div>
                    </TableCell>
                    <TableCell>
                      <code className="block max-w-md break-all font-mono text-xs leading-5 text-anvil-300">
                        adb {row.plan.args.join(" ")}
                      </code>
                    </TableCell>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </Card>
    </>
  );
}

function DeviceChoice({
  devices,
  selectedTransportId,
  selectedSerial,
  onSelect,
}: {
  devices: Device[];
  selectedTransportId: number | null;
  selectedSerial: string | null;
  onSelect: (device: Device) => void;
}) {
  const { t } = useTranslation();
  return (
    <Card className="p-4">
      <h3 className="text-sm font-semibold text-anvil-50">
        {t("common.selectDevice")}
      </h3>
      <div className="mt-3 flex flex-wrap gap-2">
        {devices.map((device) => {
          const active =
            selectedTransportId != null
              ? device.transport_id === selectedTransportId
              : device.serial === selectedSerial;
          return (
            <Button
              key={`${device.transport_id ?? device.serial}:${device.connection_generation}`}
              variant={active ? "primary" : "secondary"}
              onClick={() => onSelect(device)}
            >
              {device.model ?? device.serial}
            </Button>
          );
        })}
      </div>
    </Card>
  );
}

function WorkspaceTab({
  active,
  onClick,
  children,
}: {
  active: boolean;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      role="tab"
      aria-selected={active}
      className={`rounded-md px-4 py-2 text-sm font-medium transition ${
        active
          ? "bg-circuit-300 text-anvil-950"
          : "text-anvil-300 hover:bg-white/[0.06] hover:text-anvil-50"
      }`}
      onClick={onClick}
    >
      {children}
    </button>
  );
}

function LabeledField({
  label,
  children,
}: {
  label: string;
  children: React.ReactNode;
}) {
  return (
    <label className="grid gap-1.5 text-sm text-anvil-300">
      <span>{label}</span>
      {children}
    </label>
  );
}

function SelectField({
  value,
  onChange,
  children,
  ariaLabel,
}: {
  value: string | number;
  onChange: (value: string) => void;
  children: React.ReactNode;
  ariaLabel?: string;
}) {
  return (
    <select
      aria-label={ariaLabel}
      className="h-9 rounded-md border border-white/10 bg-anvil-900 px-3 text-sm text-anvil-50 outline-none transition hover:border-white/20 focus:border-circuit-300/60 focus:ring-2 focus:ring-circuit-300/20"
      value={value}
      onChange={(event) => onChange(event.target.value)}
    >
      {children}
    </select>
  );
}

function parseOptionalInteger(value: string): number | null {
  if (!value.trim()) return null;
  const parsed = Number(value);
  return Number.isSafeInteger(parsed) && parsed >= 0 ? parsed : null;
}

function fileSafeName(value: string): string {
  const safe = value
    .trim()
    .toLocaleLowerCase()
    .replace(/[^a-z0-9._-]+/g, "-")
    .replace(/^-+|-+$/g, "");
  return safe || "droidsmith-profile";
}

function previewTone(
  status: ProfilePreviewStatus,
): "neutral" | "success" | "warning" {
  if (status === "already_matches") return "success";
  if (status === "missing") return "warning";
  return "neutral";
}
