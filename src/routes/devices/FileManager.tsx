import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import {
  errorMessage,
  callApplyRemoteFileMutation,
  callCancelOperation,
  callListRemoteFiles,
  callPullFile,
  callPushFile,
  callPlanRemoteFileMutation,
  callSelectHostPath,
  newOperationId,
  type DeviceTarget,
  type HostPathGrant,
  type OperationEvent,
  type RemoteFileEntry,
  type RemoteFileMutationPlan,
  type RemoteFileMutationRequest,
  type RemoteListing,
} from "../../lib/tauri";
import { useFocusTrap } from "../../lib/useFocusTrap";
import {
  Badge,
  Button,
  Card,
  EmptyState,
  FieldInput,
  RevealInFolderButton,
} from "../common";
import {
  formatBytes,
  formatKb,
  statusToneClass,
  type StatusMessage,
} from "./common";

type FileNameDraft =
  | { kind: "mkdir"; value: string }
  | { kind: "rename"; entry: RemoteFileEntry; value: string };

type FileReview =
  | {
      kind: "mutation";
      request: RemoteFileMutationRequest;
      plan: RemoteFileMutationPlan;
    }
  | { kind: "push"; grant: HostPathGrant; remotePath: string };

/** Remote file browser + push/pull/rename/delete for the selected device
 *  (IMP-67: extracted verbatim from the former Devices.tsx god-file). */
export function FileManager({ target }: { target: DeviceTarget }) {
  const { t } = useTranslation();
  const [listing, setListing] = useState<RemoteListing | null>(null);
  const [currentPath, setCurrentPath] = useState("/sdcard");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [draft, setDraft] = useState<FileNameDraft | null>(null);
  const [review, setReview] = useState<FileReview | null>(null);
  const [reviewBusy, setReviewBusy] = useState(false);
  const [operationMessage, setOperationMessage] = useState<string | null>(null);
  const [fileOperationId, setFileOperationId] = useState<string | null>(null);
  const [pullMsg, setPullMsg] = useState<StatusMessage>(null);
  const [pullPath, setPullPath] = useState<string | null>(null);
  const [pullOperationId, setPullOperationId] = useState<string | null>(null);
  const pullOperationRef = useRef<string | null>(null);
  const pullGenerationRef = useRef(0);
  const fileOperationRef = useRef<string | null>(null);
  const draftTrapRef = useFocusTrap<HTMLDivElement>(draft !== null);
  const reviewTrapRef = useFocusTrap<HTMLDivElement>(review !== null);

  useEffect(() => {
    return () => {
      pullGenerationRef.current += 1;
      const operationId = pullOperationRef.current;
      pullOperationRef.current = null;
      if (operationId) void callCancelOperation(operationId);
      const fileOperationId = fileOperationRef.current;
      fileOperationRef.current = null;
      if (fileOperationId) void callCancelOperation(fileOperationId);
    };
  }, [target.serial, target.transport_id, target.connection_generation]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || reviewBusy) return;
      if (review) setReview(null);
      else if (draft) setDraft(null);
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [draft, review, reviewBusy]);

  const remotePathFor = useCallback(
    (name: string) =>
      currentPath === "/" ? `/${name}` : `${currentPath}/${name}`,
    [currentPath],
  );

  const browse = useCallback(
    async (path: string) => {
      setLoading(true);
      setPullMsg(null);
      // Clear any prior file-operation status so a "complete" line does not
      // linger after the user navigates to another directory. confirmReview
      // sets its message after awaiting the refresh browse(), so the freshly
      // completed operation still reports.
      setOperationMessage(null);
      setError(null);
      try {
        const result = await callListRemoteFiles(target, path);
        setListing(result);
        setCurrentPath(path);
      } catch (e) {
        setListing(null);
        setError(errorMessage(e));
      } finally {
        setLoading(false);
      }
    },
    [target],
  );

  const navigateUp = useCallback(() => {
    const parent = currentPath.replace(/\/[^/]+\/?$/, "") || "/";
    void browse(parent);
  }, [currentPath, browse]);

  const pullRemote = useCallback(
    async (entry: RemoteFileEntry) => {
      let operationId: string | null = null;
      let generation: number | null = null;
      try {
        const pathGrant = await callSelectHostPath(
          "pull_save",
          entry.name.replace(/[<>:"/\\|?*]/gu, "_"),
        );
        if (!pathGrant) {
          setPullMsg(null);
          return;
        }
        setPullPath(null);
        setPullMsg({
          text: t("devices.controls.pulling", { name: entry.name }),
          tone: "neutral",
        });
        const remoteFull =
          currentPath === "/"
            ? `/${entry.name}`
            : `${currentPath}/${entry.name}`;
        operationId = newOperationId("pull");
        generation = pullGenerationRef.current + 1;
        pullGenerationRef.current = generation;
        pullOperationRef.current = operationId;
        setPullOperationId(operationId);
        const artifact = await callPullFile(target, remoteFull, pathGrant.id, {
          operationId,
          onEvent: (event: OperationEvent) => {
            if (
              pullOperationRef.current !== operationId ||
              pullGenerationRef.current !== generation
            )
              return;
            if (event.kind === "progress") {
              setPullMsg({
                tone: "neutral",
                text: t("devices.controls.pullProgress", {
                  name: entry.name,
                  seconds: Math.max(
                    1,
                    Math.round((event.elapsed_ms ?? 0) / 1000),
                  ),
                }),
              });
            }
          },
        });
        if (pullGenerationRef.current !== generation) return;
        pullOperationRef.current = null;
        setPullOperationId(null);
        setPullMsg({
          tone: "success",
          text: t("devices.controls.savedName", {
            name: entry.name,
            path: artifact.local_path,
          }),
        });
        setPullPath(artifact.local_path);
      } catch (e) {
        if (
          operationId &&
          (pullGenerationRef.current !== generation ||
            pullOperationRef.current !== operationId)
        )
          return;
        pullOperationRef.current = null;
        setPullOperationId(null);
        setPullPath(null);
        setPullMsg({
          tone: "danger",
          text: t("devices.controls.failed", {
            message: errorMessage(e),
          }),
        });
      }
    },
    [target, currentPath, t],
  );

  const cancelPull = useCallback(async () => {
    const operationId = pullOperationRef.current;
    if (!operationId) return;
    setPullMsg({
      text: t("devices.controls.pullCancelling"),
      tone: "neutral",
    });
    await callCancelOperation(operationId);
  }, [t]);

  const stageMutation = useCallback(
    async (request: RemoteFileMutationRequest) => {
      setOperationMessage(null);
      try {
        const plan = await callPlanRemoteFileMutation(request);
        setReview({ kind: "mutation", request, plan });
      } catch (e) {
        setOperationMessage(
          t("devices.controls.fileOperationFailed", {
            message: errorMessage(e),
          }),
        );
      }
    },
    [t],
  );

  const submitDraft = useCallback(async () => {
    if (!draft) return;
    const name = draft.value.trim();
    if (
      !name ||
      name === "." ||
      name === ".." ||
      name.includes("/") ||
      name.includes("\\")
    ) {
      setOperationMessage(t("devices.controls.invalidFileName"));
      return;
    }
    const request: RemoteFileMutationRequest =
      draft.kind === "mkdir"
        ? {
            kind: "mkdir",
            source_path: remotePathFor(name),
            destination_path: null,
          }
        : {
            kind: "rename",
            source_path: remotePathFor(draft.entry.name),
            destination_path: remotePathFor(name),
          };
    setDraft(null);
    await stageMutation(request);
  }, [draft, remotePathFor, stageMutation, t]);

  const stagePush = useCallback(async () => {
    setOperationMessage(null);
    try {
      const grant = await callSelectHostPath("push_open");
      if (!grant) return;
      const fileName = grant.local_path.split(/[\\/]/u).pop()?.trim();
      if (!fileName) {
        setOperationMessage(t("devices.controls.invalidFileName"));
        return;
      }
      setReview({
        kind: "push",
        grant,
        remotePath: remotePathFor(fileName),
      });
    } catch (e) {
      setOperationMessage(
        t("devices.controls.fileOperationFailed", {
          message: errorMessage(e),
        }),
      );
    }
  }, [remotePathFor, t]);

  const confirmReview = useCallback(async () => {
    if (!review) return;
    setReviewBusy(true);
    setOperationMessage(t("devices.controls.applyingFileOperation"));
    let operationId: string | null = null;
    try {
      if (review.kind === "mutation") {
        await callApplyRemoteFileMutation(target, review.request, true);
      } else {
        operationId = newOperationId("push");
        fileOperationRef.current = operationId;
        setFileOperationId(operationId);
        await callPushFile(target, review.grant.id, review.remotePath, true, {
          operationId,
          onEvent: (event: OperationEvent) => {
            if (fileOperationRef.current !== operationId) return;
            if (event.kind === "progress") {
              setOperationMessage(
                t("devices.controls.pushProgress", {
                  seconds: Math.max(
                    1,
                    Math.round((event.elapsed_ms ?? 0) / 1000),
                  ),
                }),
              );
            }
          },
        });
      }
      fileOperationRef.current = null;
      setFileOperationId(null);
      setReview(null);
      await browse(currentPath);
      setOperationMessage(t("devices.controls.fileOperationComplete"));
    } catch (e) {
      if (operationId && fileOperationRef.current !== operationId) return;
      fileOperationRef.current = null;
      setFileOperationId(null);
      setOperationMessage(
        t("devices.controls.fileOperationFailed", {
          message: errorMessage(e),
        }),
      );
    } finally {
      setReviewBusy(false);
    }
  }, [browse, currentPath, review, t, target]);

  const cancelFileOperation = useCallback(async () => {
    const operationId = fileOperationRef.current;
    if (!operationId) return;
    setOperationMessage(t("devices.controls.pushCancelling"));
    await callCancelOperation(operationId);
  }, [t]);

  return (
    <>
      {draft && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4 backdrop-blur-sm">
          <div
            ref={draftTrapRef}
            role="dialog"
            aria-modal="true"
            aria-labelledby="file-name-dialog-title"
            tabIndex={-1}
            className="w-full max-w-md rounded-lg border border-white/10 bg-anvil-950 p-5 shadow-2xl outline-none"
          >
            <h3
              id="file-name-dialog-title"
              className="text-lg font-semibold text-anvil-50"
            >
              {draft.kind === "mkdir"
                ? t("devices.controls.newFolderTitle")
                : t("devices.controls.renameTitle")}
            </h3>
            <p className="mt-2 text-sm text-anvil-300">
              {t("devices.controls.fileNameBody", { path: currentPath })}
            </p>
            <label className="mt-4 block text-xs font-medium text-anvil-300">
              {t("devices.controls.fileName")}
              <FieldInput
                autoFocus
                className="mt-2 w-full font-mono"
                value={draft.value}
                onChange={(event) =>
                  setDraft({ ...draft, value: event.currentTarget.value })
                }
                onKeyDown={(event) => {
                  if (event.key === "Enter") void submitDraft();
                }}
              />
            </label>
            <div className="mt-5 flex justify-end gap-2">
              <Button type="button" onClick={() => setDraft(null)}>
                {t("common.cancel")}
              </Button>
              <Button
                type="button"
                variant="primary"
                onClick={() => void submitDraft()}
              >
                {t("devices.controls.reviewFileOperation")}
              </Button>
            </div>
          </div>
        </div>
      )}

      {review && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4 backdrop-blur-sm">
          <div
            ref={reviewTrapRef}
            role="alertdialog"
            aria-modal="true"
            aria-labelledby="file-review-title"
            aria-describedby="file-review-body"
            tabIndex={-1}
            className="max-h-[90vh] w-full max-w-xl overflow-y-auto rounded-lg border border-white/10 bg-anvil-950 p-5 shadow-2xl outline-none"
          >
            <Badge
              tone={
                review.kind === "mutation" && review.plan.destructive
                  ? "danger"
                  : "warning"
              }
            >
              {t("devices.controls.fileChange")}
            </Badge>
            <h3
              id="file-review-title"
              className="mt-4 text-lg font-semibold text-anvil-50"
            >
              {t("devices.controls.reviewFileOperation")}
            </h3>
            <p
              id="file-review-body"
              className="mt-2 text-sm leading-6 text-anvil-300"
            >
              {review.kind === "mutation" && review.plan.destructive
                ? t("devices.controls.destructiveFileWarning")
                : t("devices.controls.fileReviewBody")}
            </p>
            <dl className="mt-4 space-y-3 text-xs">
              <div>
                <dt className="text-anvil-500">
                  {t("devices.controls.source")}
                </dt>
                <dd className="mt-1 break-all font-mono text-anvil-100">
                  {review.kind === "push"
                    ? review.grant.local_path
                    : review.plan.source_path}
                </dd>
              </div>
              {(review.kind === "push" || review.plan.destination_path) && (
                <div>
                  <dt className="text-anvil-500">
                    {t("devices.controls.target")}
                  </dt>
                  <dd className="mt-1 break-all font-mono text-anvil-100">
                    {review.kind === "push"
                      ? review.remotePath
                      : review.plan.destination_path}
                  </dd>
                </div>
              )}
            </dl>
            <pre className="mt-4 overflow-auto whitespace-pre-wrap break-all rounded-md border border-white/10 bg-black/30 p-3 font-mono text-xs text-anvil-100">
              {review.kind === "push"
                ? `adb push ${JSON.stringify(review.grant.local_path)} ${JSON.stringify(review.remotePath)}`
                : `adb shell ${review.plan.argv.map((arg) => JSON.stringify(arg)).join(" ")}`}
            </pre>
            <div className="mt-5 flex justify-end gap-2">
              <Button
                type="button"
                onClick={() => setReview(null)}
                disabled={reviewBusy}
              >
                {t("common.cancel")}
              </Button>
              <Button
                type="button"
                variant={
                  review.kind === "mutation" && review.plan.destructive
                    ? "danger"
                    : "primary"
                }
                onClick={() => void confirmReview()}
                disabled={reviewBusy}
              >
                {reviewBusy
                  ? t("devices.controls.applyingFileOperation")
                  : t("devices.controls.confirmFileOperation")}
              </Button>
            </div>
          </div>
        </div>
      )}

      <Card className="overflow-hidden p-0">
        <div className="flex flex-col gap-3 border-b border-white/10 p-4 sm:flex-row sm:items-center sm:justify-between">
          <div>
            <h3 className="text-sm font-semibold text-anvil-50">
              {t("devices.controls.fileManager")}
            </h3>
            <p className="mt-1 text-xs text-anvil-400">
              {t("devices.controls.fileManagerBody")}
            </p>
          </div>
          <div className="flex items-center gap-2">
            {listing?.free_space_kb != null && (
              <Badge tone="neutral">
                {t("devices.controls.freeSpace", {
                  size: formatKb(listing.free_space_kb),
                })}
              </Badge>
            )}
            {listing && (
              <>
                <Button
                  type="button"
                  size="sm"
                  onClick={() => void stagePush()}
                >
                  {t("devices.controls.push")}
                </Button>
                <Button
                  type="button"
                  size="sm"
                  onClick={() => setDraft({ kind: "mkdir", value: "" })}
                >
                  {t("devices.controls.newFolder")}
                </Button>
              </>
            )}
            <Button
              type="button"
              size="sm"
              variant="primary"
              onClick={() => void browse(currentPath)}
              disabled={loading}
            >
              {loading
                ? t("devices.controls.loading")
                : listing
                  ? t("devices.controls.refresh")
                  : t("devices.controls.browse")}
            </Button>
          </div>
        </div>

        {error && (
          <div className="border-b border-red-500/20 bg-red-500/10 px-4 py-3 text-xs text-red-200">
            {t("devices.controls.fileListFailed", {
              path: currentPath,
              message: error,
            })}
          </div>
        )}

        {!listing && !loading && !error && (
          <EmptyState title={t("devices.controls.noDirectory")}>
            <p>
              {t("devices.controls.noDirectoryBodyPrefix")} <code>/sdcard</code>{" "}
              {t("devices.controls.noDirectoryBodySuffix")}
            </p>
          </EmptyState>
        )}

        {listing && (
          <>
            <div className="flex items-center gap-2 border-b border-white/10 bg-white/[0.02] px-4 py-2">
              <Button
                type="button"
                size="sm"
                variant="ghost"
                onClick={navigateUp}
                disabled={currentPath === "/"}
              >
                ..
              </Button>
              <code className="flex-1 truncate font-mono text-xs text-anvil-200">
                {currentPath}
              </code>
            </div>
            <div className="max-h-80 divide-y divide-white/5 overflow-y-auto">
              {listing.entries.length === 0 && (
                <EmptyState
                  title={t("devices.controls.emptyDirectory")}
                  className="border-t-0"
                >
                  <p>{t("devices.controls.emptyDirectoryBody")}</p>
                </EmptyState>
              )}
              {listing.entries.map((entry, index) => (
                <div
                  key={`${entry.name}-${index}`}
                  className="flex items-center gap-3 px-4 py-2 text-xs hover:bg-white/[0.03]"
                >
                  <FileGlyph directory={entry.is_dir} />
                  {entry.is_dir ? (
                    <button
                      type="button"
                      className="min-w-0 flex-1 truncate text-start font-mono text-circuit-200 hover:underline"
                      onClick={() =>
                        void browse(
                          currentPath === "/"
                            ? `/${entry.name}`
                            : `${currentPath}/${entry.name}`,
                        )
                      }
                    >
                      {entry.name}/
                    </button>
                  ) : (
                    <span className="min-w-0 flex-1 truncate font-mono text-anvil-100">
                      {entry.name}
                    </span>
                  )}
                  <span className="shrink-0 font-mono text-anvil-500">
                    {entry.is_dir
                      ? ""
                      : formatBytes(entry.size, t("common.unknown"))}
                  </span>
                  <span className="hidden shrink-0 font-mono text-anvil-600 sm:inline">
                    {entry.permissions}
                  </span>
                  {entry.parse_error && (
                    <Badge tone="warning" className="shrink-0">
                      {t("devices.controls.parseIssue")}
                    </Badge>
                  )}
                  {!entry.is_dir && !entry.parse_error && (
                    <Button
                      type="button"
                      size="sm"
                      variant="ghost"
                      onClick={() => void pullRemote(entry)}
                    >
                      {t("devices.controls.pull")}
                    </Button>
                  )}
                  {!entry.parse_error && (
                    <>
                      <Button
                        type="button"
                        size="sm"
                        variant="ghost"
                        onClick={() =>
                          setDraft({ kind: "rename", entry, value: entry.name })
                        }
                      >
                        {t("devices.controls.rename")}
                      </Button>
                      <Button
                        type="button"
                        size="sm"
                        variant="danger"
                        onClick={() =>
                          void stageMutation({
                            kind: entry.is_dir
                              ? "delete_directory"
                              : "delete_file",
                            source_path: remotePathFor(entry.name),
                            destination_path: null,
                          })
                        }
                      >
                        {t("devices.controls.delete")}
                      </Button>
                    </>
                  )}
                </div>
              ))}
            </div>
            {pullMsg && (
              <div className="flex items-center justify-between gap-3 border-t border-white/10 px-4 py-2">
                <p className={`text-xs ${statusToneClass(pullMsg.tone)}`}>
                  {pullMsg.text}
                </p>
                {pullOperationId ? (
                  <Button
                    type="button"
                    size="sm"
                    variant="danger"
                    onClick={() => void cancelPull()}
                  >
                    {t("common.cancel")}
                  </Button>
                ) : (
                  pullPath && <RevealInFolderButton path={pullPath} />
                )}
              </div>
            )}
            {operationMessage && (
              <div
                role="status"
                className="flex items-center justify-between gap-3 border-t border-white/10 px-4 py-2"
              >
                <p className="text-xs text-anvil-300">{operationMessage}</p>
                {fileOperationId && (
                  <Button
                    type="button"
                    size="sm"
                    variant="danger"
                    onClick={() => void cancelFileOperation()}
                  >
                    {t("common.cancel")}
                  </Button>
                )}
              </div>
            )}
          </>
        )}
      </Card>
    </>
  );
}

function FileGlyph({ directory }: { directory: boolean }) {
  return (
    <svg
      viewBox="0 0 24 24"
      className="h-4 w-4 shrink-0 text-anvil-400"
      fill="none"
      stroke="currentColor"
      strokeLinecap="round"
      strokeLinejoin="round"
      strokeWidth="1.8"
      aria-hidden="true"
    >
      {directory ? (
        <>
          <path d="M3.5 6.5h6l2 2H20a1.5 1.5 0 0 1 1.5 1.5v7.5A1.5 1.5 0 0 1 20 19H4a1.5 1.5 0 0 1-1.5-1.5V8A1.5 1.5 0 0 1 4 6.5Z" />
          <path d="M3.5 10h18" />
        </>
      ) : (
        <>
          <path d="M7 3.5h7l3 3V20a.5.5 0 0 1-.5.5h-9A.5.5 0 0 1 7 20V3.5Z" />
          <path d="M14 3.5v3h3" />
          <path d="M9.5 11h5M9.5 14h5" />
        </>
      )}
    </svg>
  );
}
