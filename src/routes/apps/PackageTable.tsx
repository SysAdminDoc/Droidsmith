import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type ReactNode,
} from "react";
import { useTranslation } from "react-i18next";

import { cn } from "../../lib/cn";
import { formatBytes } from "../../lib/format";
import {
  type ActionKind,
  type AppPackage,
  type AppPackageMetadata,
} from "../../lib/tauri";
import {
  Badge,
  Button,
  Card,
  EmptyState,
  TableCell,
  TableHeaderCell,
} from "../common";

// IMP-62: roving-tabindex navigation for the package data grid (W3C ARIA APG
// grid pattern). Row 0 is the column-header row; body rows follow. Exactly one
// cell is the tab stop at a time; arrow keys move it, Home/End jump within a
// row (Ctrl to the grid corners), PageUp/Down move ten rows, and Enter/Space
// hands focus to the first interactive control inside the focused cell (Escape
// returns focus to the cell).
const GRID_PAGE_STEP = 10;
const VIRTUALIZATION_THRESHOLD = 200;
const VIRTUAL_ROW_HEIGHT = 88;
const VIRTUAL_VIEWPORT_HEIGHT = 560;
const VIRTUAL_OVERSCAN = 8;
type PackageSortKey = "package" | "type" | "state";

function comparePackages(
  left: AppPackage,
  right: AppPackage,
  sortBy: PackageSortKey,
): number {
  if (sortBy === "type") {
    const typeOrder = Number(left.system) - Number(right.system);
    if (typeOrder !== 0) return typeOrder;
  } else if (sortBy === "state") {
    const stateOrder = packageStateRank(left) - packageStateRank(right);
    if (stateOrder !== 0) return stateOrder;
  }
  return left.package.localeCompare(right.package);
}

function packageStateRank(pkg: AppPackage): number {
  if (pkg.retained) return 3;
  if (pkg.archived) return 2;
  return pkg.enabled ? 0 : 1;
}

function useRovingGrid(rowCount: number, colCount: number) {
  const gridRef = useRef<HTMLTableElement>(null);
  const [active, setActive] = useState({ row: 0, col: 0 });

  const focusCell = useCallback(
    (row: number, col: number) => {
      const r = Math.max(0, Math.min(rowCount - 1, row));
      const c = Math.max(0, Math.min(colCount - 1, col));
      setActive({ row: r, col: c });
      gridRef.current
        ?.querySelector<HTMLElement>(`[data-grid-cell="${r}-${c}"]`)
        ?.focus();
    },
    [rowCount, colCount],
  );

  // A shrinking result set must not strand the tab stop on a removed row.
  useEffect(() => {
    setActive((current) => ({
      row: Math.max(0, Math.min(current.row, rowCount - 1)),
      col: Math.max(0, Math.min(current.col, colCount - 1)),
    }));
  }, [rowCount, colCount]);

  const onKeyDown = useCallback(
    (event: ReactKeyboardEvent) => {
      const target = event.target as HTMLElement;
      const onCell = target.hasAttribute?.("data-grid-cell");
      const { row, col } = active;
      switch (event.key) {
        case "ArrowRight":
          if (onCell) {
            event.preventDefault();
            focusCell(row, col + 1);
          }
          break;
        case "ArrowLeft":
          if (onCell) {
            event.preventDefault();
            focusCell(row, col - 1);
          }
          break;
        case "ArrowDown":
          if (onCell) {
            event.preventDefault();
            focusCell(row + 1, col);
          }
          break;
        case "ArrowUp":
          if (onCell) {
            event.preventDefault();
            focusCell(row - 1, col);
          }
          break;
        case "Home":
          if (onCell) {
            event.preventDefault();
            focusCell(event.ctrlKey ? 0 : row, 0);
          }
          break;
        case "End":
          if (onCell) {
            event.preventDefault();
            focusCell(event.ctrlKey ? rowCount - 1 : row, colCount - 1);
          }
          break;
        case "PageDown":
          if (onCell) {
            event.preventDefault();
            focusCell(row + GRID_PAGE_STEP, col);
          }
          break;
        case "PageUp":
          if (onCell) {
            event.preventDefault();
            focusCell(row - GRID_PAGE_STEP, col);
          }
          break;
        case "Enter":
        case " ":
          if (onCell) {
            const widget = target.querySelector<HTMLElement>(
              "button, input, a[href], select, textarea",
            );
            if (widget) {
              event.preventDefault();
              widget.focus();
            }
          }
          break;
        case "Escape":
          if (!onCell) {
            target.closest<HTMLElement>("[data-grid-cell]")?.focus();
          }
          break;
        default:
          break;
      }
    },
    [active, focusCell, rowCount, colCount],
  );

  const cellProps = useCallback(
    (row: number, col: number) => ({
      "data-grid-cell": `${row}-${col}`,
      role: row === 0 ? ("columnheader" as const) : ("gridcell" as const),
      tabIndex: active.row === row && active.col === col ? 0 : -1,
      "aria-colindex": col + 1,
    }),
    [active],
  );

  return { gridRef, onKeyDown, cellProps };
}

/** Installed-package ARIA grid with roving-tabindex navigation and per-row
 *  actions (IMP-67: extracted verbatim from the former Apps.tsx god-file). */
export function PackageTable({
  packages,
  metadata,
  totalCount,
  archiveSupported,
  suspendSupported,
  unsuspendSupported,
  hideSupported,
  unhideSupported,
  unstopSupported,
  disableUntilUsedSupported,
  defaultStateSupported,
  suspendQuarantineSupported,
  selectedPackages,
  onToggleSelected,
  onToggleAll,
  onMetadataRequest,
  onAction,
  onInspect,
  onExport,
  onLegacyExport,
  showLegacyExport,
}: {
  packages: AppPackage[];
  metadata: Record<string, AppPackageMetadata | null>;
  totalCount: number;
  archiveSupported: boolean;
  suspendSupported: boolean;
  unsuspendSupported: boolean;
  hideSupported: boolean;
  unhideSupported: boolean;
  unstopSupported: boolean;
  disableUntilUsedSupported: boolean;
  defaultStateSupported: boolean;
  suspendQuarantineSupported: boolean;
  selectedPackages: Set<string>;
  onToggleSelected: (pkg: string) => void;
  onToggleAll: () => void;
  onMetadataRequest: (pkg: string) => void;
  onAction: (pkg: string, kind: ActionKind) => void;
  onInspect: (pkg: string) => void;
  onExport: (pkg: string) => void;
  onLegacyExport: (pkg: string) => void;
  showLegacyExport: boolean;
}) {
  const { t } = useTranslation();
  const [sortBy, setSortBy] = useState<PackageSortKey>("package");
  const scrollRef = useRef<HTMLDivElement>(null);
  const [scrollTop, setScrollTop] = useState(0);
  const allVisibleSelected =
    packages.length > 0 &&
    packages.every((pkg) => selectedPackages.has(pkg.package));
  const sortedPackages = useMemo(
    () =>
      [...packages].sort((left, right) => comparePackages(left, right, sortBy)),
    [packages, sortBy],
  );
  const virtualized = sortedPackages.length > VIRTUALIZATION_THRESHOLD;
  const firstVisibleIndex = virtualized
    ? Math.max(0, Math.floor(scrollTop / VIRTUAL_ROW_HEIGHT) - VIRTUAL_OVERSCAN)
    : 0;
  const visibleRowCount = virtualized
    ? Math.ceil(VIRTUAL_VIEWPORT_HEIGHT / VIRTUAL_ROW_HEIGHT) +
      VIRTUAL_OVERSCAN * 2
    : sortedPackages.length;
  const visiblePackages = virtualized
    ? sortedPackages
        .slice(firstVisibleIndex, firstVisibleIndex + visibleRowCount)
        .map((pkg, offset) => ({ pkg, rowIndex: firstVisibleIndex + offset }))
    : sortedPackages.map((pkg, rowIndex) => ({ pkg, rowIndex }));

  useEffect(() => {
    setScrollTop(0);
    if (scrollRef.current) scrollRef.current.scrollTop = 0;
  }, [packages.length]);
  const gridColumnCount = 5;
  const { gridRef, onKeyDown, cellProps } = useRovingGrid(
    packages.length + 1,
    gridColumnCount,
  );

  return (
    <Card className="overflow-hidden p-0">
      <div className="flex flex-col gap-2 border-b border-white/10 px-3 py-2.5 sm:flex-row sm:items-center sm:justify-between">
        <div>
          <h3 className="text-sm font-semibold text-anvil-50">
            {t("apps.installedPackages")}
          </h3>
          <p className="sr-only">{t("apps.installedPackagesBody")}</p>
        </div>
        <div className="flex gap-2">
          {packages.length !== totalCount && (
            <Badge tone="info">
              {t("apps.shownCount", {
                shown: packages.length,
                total: totalCount,
              })}
            </Badge>
          )}
          <Badge tone="neutral">
            {t("common.totalCount", { count: totalCount })}
          </Badge>
        </div>
      </div>
      {packages.length === 0 ? (
        <EmptyState title={t("apps.noMatchingPackages")}>
          <p>{t("apps.noMatchingPackagesBody")}</p>
        </EmptyState>
      ) : (
        <div
          ref={scrollRef}
          onScroll={(event) => {
            if (virtualized) setScrollTop(event.currentTarget.scrollTop);
          }}
          className={
            virtualized ? "max-h-[35rem] overflow-auto" : "overflow-x-auto"
          }
          style={
            virtualized ? { maxHeight: VIRTUAL_VIEWPORT_HEIGHT } : undefined
          }
        >
          <table
            ref={gridRef}
            role="grid"
            aria-label={t("apps.installedPackages")}
            aria-rowcount={packages.length + 1}
            aria-colcount={gridColumnCount}
            aria-multiselectable="true"
            onKeyDown={onKeyDown}
            className="min-w-full text-sm"
          >
            <thead className="bg-white/[0.04]">
              <tr role="row" aria-rowindex={1}>
                <TableHeaderCell {...cellProps(0, 0)}>
                  <input
                    type="checkbox"
                    checked={allVisibleSelected}
                    onChange={onToggleAll}
                    disabled={packages.length === 0}
                    aria-label={t("apps.selectAllPackages")}
                    className="h-4 w-4 accent-circuit-400"
                  />
                </TableHeaderCell>
                <TableHeaderCell
                  {...cellProps(0, 1)}
                  aria-sort={sortBy === "package" ? "ascending" : "none"}
                >
                  <button
                    type="button"
                    onClick={() => setSortBy("package")}
                    aria-label={t("apps.package")}
                    className="font-semibold hover:text-anvil-100"
                  >
                    {t("apps.package")}
                    {sortBy === "package" && (
                      <span className="ml-1" aria-hidden="true">
                        ↑
                      </span>
                    )}
                  </button>
                </TableHeaderCell>
                <TableHeaderCell
                  {...cellProps(0, 2)}
                  aria-sort={sortBy === "type" ? "ascending" : "none"}
                >
                  <button
                    type="button"
                    onClick={() => setSortBy("type")}
                    aria-label={t("apps.type")}
                    className="font-semibold hover:text-anvil-100"
                  >
                    {t("apps.type")}
                    {sortBy === "type" && (
                      <span className="ml-1" aria-hidden="true">
                        ↑
                      </span>
                    )}
                  </button>
                </TableHeaderCell>
                <TableHeaderCell
                  {...cellProps(0, 3)}
                  aria-sort={sortBy === "state" ? "ascending" : "none"}
                >
                  <button
                    type="button"
                    onClick={() => setSortBy("state")}
                    aria-label={t("devices.state")}
                    className="font-semibold hover:text-anvil-100"
                  >
                    {t("devices.state")}
                    {sortBy === "state" && (
                      <span className="ml-1" aria-hidden="true">
                        ↑
                      </span>
                    )}
                  </button>
                </TableHeaderCell>
                <TableHeaderCell {...cellProps(0, 4)}>
                  {t("apps.actions")}
                </TableHeaderCell>
              </tr>
            </thead>
            <tbody className="divide-y divide-white/10">
              {firstVisibleIndex > 0 && (
                <tr aria-hidden="true" role="presentation">
                  <td colSpan={gridColumnCount}>
                    <div
                      aria-hidden="true"
                      style={{ height: firstVisibleIndex * VIRTUAL_ROW_HEIGHT }}
                    />
                  </td>
                </tr>
              )}
              {visiblePackages.map(({ pkg, rowIndex }) => (
                <tr
                  key={pkg.package}
                  data-package-row="true"
                  role="row"
                  aria-rowindex={rowIndex + 2}
                  aria-selected={selectedPackages.has(pkg.package)}
                  className="h-[5.5rem] bg-anvil-950/20 transition hover:bg-white/[0.035]"
                >
                  <TableCell {...cellProps(rowIndex + 1, 0)}>
                    <input
                      type="checkbox"
                      checked={selectedPackages.has(pkg.package)}
                      onChange={() => onToggleSelected(pkg.package)}
                      aria-label={t("apps.selectPackage", {
                        package: pkg.package,
                      })}
                      className="h-4 w-4 accent-circuit-400"
                    />
                  </TableCell>
                  <TableCell {...cellProps(rowIndex + 1, 1)}>
                    <PackageIdentity
                      pkg={pkg}
                      metadata={metadata[pkg.package]}
                      onRequest={onMetadataRequest}
                    />
                  </TableCell>
                  <TableCell {...cellProps(rowIndex + 1, 2)}>
                    <Badge tone={pkg.system ? "warning" : "neutral"}>
                      {pkg.system
                        ? t("apps.filterSystem")
                        : t("apps.filterUser")}
                    </Badge>
                  </TableCell>
                  <TableCell {...cellProps(rowIndex + 1, 3)}>
                    <Badge
                      tone={
                        pkg.retained
                          ? "neutral"
                          : pkg.archived
                            ? "warning"
                            : pkg.enabled
                              ? "success"
                              : "danger"
                      }
                    >
                      {pkg.retained
                        ? t("apps.retainedLabel")
                        : pkg.archived
                          ? t("apps.filterArchived")
                          : pkg.enabled
                            ? t("apps.filterEnabled")
                            : t("apps.filterDisabled")}
                    </Badge>
                  </TableCell>
                  <TableCell {...cellProps(rowIndex + 1, 4)}>
                    <div className="flex min-w-[9rem] items-center justify-end gap-1.5">
                      {pkg.retained ? (
                        <Button
                          type="button"
                          size="sm"
                          variant="danger"
                          onClick={() =>
                            onAction(pkg.package, "uninstall_for_user")
                          }
                        >
                          {t("apps.removeRetainedData")}
                        </Button>
                      ) : pkg.archived ? (
                        <Button
                          type="button"
                          size="sm"
                          variant="primary"
                          onClick={() =>
                            onAction(pkg.package, "request_unarchive")
                          }
                        >
                          {t("apps.unarchive")}
                        </Button>
                      ) : pkg.enabled ? (
                        <Button
                          type="button"
                          size="sm"
                          onClick={() =>
                            onAction(
                              pkg.package,
                              suspendSupported ? "suspend" : "disable",
                            )
                          }
                        >
                          {t(
                            suspendSupported ? "apps.suspend" : "apps.disable",
                          )}
                        </Button>
                      ) : (
                        <Button
                          type="button"
                          size="sm"
                          variant="primary"
                          onClick={() => onAction(pkg.package, "enable")}
                        >
                          {t("apps.enable")}
                        </Button>
                      )}
                      {!pkg.archived && !pkg.retained && (
                        <PackageActionMenu
                          pkg={pkg}
                          archiveSupported={archiveSupported}
                          suspendSupported={suspendSupported}
                          unsuspendSupported={unsuspendSupported}
                          hideSupported={hideSupported}
                          unhideSupported={unhideSupported}
                          unstopSupported={unstopSupported}
                          disableUntilUsedSupported={disableUntilUsedSupported}
                          defaultStateSupported={defaultStateSupported}
                          suspendQuarantineSupported={
                            suspendQuarantineSupported
                          }
                          showLegacyExport={showLegacyExport}
                          onAction={onAction}
                          onInspect={onInspect}
                          onExport={onExport}
                          onLegacyExport={onLegacyExport}
                        />
                      )}
                    </div>
                  </TableCell>
                </tr>
              ))}
              {firstVisibleIndex + visiblePackages.length <
                sortedPackages.length && (
                <tr aria-hidden="true" role="presentation">
                  <td colSpan={gridColumnCount}>
                    <div
                      aria-hidden="true"
                      style={{
                        height:
                          (sortedPackages.length -
                            firstVisibleIndex -
                            visiblePackages.length) *
                          VIRTUAL_ROW_HEIGHT,
                      }}
                    />
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
      )}
    </Card>
  );
}

function PackageIdentity({
  pkg,
  metadata,
  onRequest,
}: {
  pkg: AppPackage;
  metadata: AppPackageMetadata | null | undefined;
  onRequest: (pkg: string) => void;
}) {
  const { t } = useTranslation();
  const containerRef = useRef<HTMLDivElement>(null);
  const label = metadata?.label ?? packageFallbackLabel(pkg.package);

  useEffect(() => {
    const element = containerRef.current;
    if (!element || metadata !== undefined || pkg.archived) return;
    if (typeof IntersectionObserver === "undefined") {
      onRequest(pkg.package);
      return;
    }
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((entry) => entry.isIntersecting)) {
          onRequest(pkg.package);
          observer.disconnect();
        }
      },
      { rootMargin: "320px 0px" },
    );
    observer.observe(element);
    return () => observer.disconnect();
  }, [metadata, onRequest, pkg.archived, pkg.package]);

  return (
    <div ref={containerRef} className="flex min-w-[18rem] items-center gap-3">
      <PackageIcon label={label} iconDataUri={metadata?.icon_data_uri} />
      <div className="min-w-0">
        <p className="truncate text-sm font-medium text-anvil-50">{label}</p>
        <code className="mt-1 block truncate font-mono text-xs text-anvil-300">
          {pkg.package}
        </code>
        {pkg.installer && (
          <p className="mt-1 text-xs text-anvil-500">
            {t("apps.viaInstaller", { installer: pkg.installer })}
          </p>
        )}
        <PackageStorageLine metadata={metadata} />
      </div>
    </div>
  );
}

/** Per-package storage, on the same lazily-loaded path as the label and icon.
 *
 *  Renders nothing until the row's metadata arrives, so sizes never delay the
 *  row; once it has, a device that does not advertise
 *  `pm get-package-storage-stats` says so rather than showing an APK-size
 *  estimate dressed up as a measurement. */
function PackageStorageLine({
  metadata,
}: {
  metadata: AppPackageMetadata | null | undefined;
}) {
  const { t, i18n } = useTranslation();
  if (!metadata) return null;
  if (!metadata.storage) {
    return (
      <p className="mt-1 text-xs text-anvil-500">
        {t("apps.storageUnavailable")}
      </p>
    );
  }
  const { code_bytes, data_bytes, cache_bytes } = metadata.storage;
  return (
    <p className="mt-1 text-xs text-anvil-400">
      {t("apps.storageBreakdown", {
        app: formatBytes(code_bytes, i18n.language),
        data: formatBytes(data_bytes, i18n.language),
        cache: formatBytes(cache_bytes, i18n.language),
      })}
    </p>
  );
}

function PackageActionMenu({
  pkg,
  archiveSupported,
  suspendSupported,
  unsuspendSupported,
  hideSupported,
  unhideSupported,
  unstopSupported,
  disableUntilUsedSupported,
  defaultStateSupported,
  suspendQuarantineSupported,
  showLegacyExport,
  onAction,
  onInspect,
  onExport,
  onLegacyExport,
}: {
  pkg: AppPackage;
  archiveSupported: boolean;
  suspendSupported: boolean;
  unsuspendSupported: boolean;
  hideSupported: boolean;
  unhideSupported: boolean;
  unstopSupported: boolean;
  disableUntilUsedSupported: boolean;
  defaultStateSupported: boolean;
  suspendQuarantineSupported: boolean;
  showLegacyExport: boolean;
  onAction: (pkg: string, kind: ActionKind) => void;
  onInspect: (pkg: string) => void;
  onExport: (pkg: string) => void;
  onLegacyExport: (pkg: string) => void;
}) {
  const { t } = useTranslation();
  const close = (button: HTMLButtonElement) => {
    button.closest("details")?.removeAttribute("open");
  };

  return (
    <details className="group flex flex-col items-end">
      <summary
        aria-label={`${t("apps.actions")}: ${pkg.package}`}
        className="grid h-9 w-9 cursor-pointer list-none place-items-center rounded text-lg text-anvil-300 transition hover:bg-white/[0.06] hover:text-anvil-50 focus:outline-none focus-visible:ring-2 focus-visible:ring-circuit-300 [&::-webkit-details-marker]:hidden"
      >
        <span aria-hidden="true">⋮</span>
      </summary>
      <div
        role="menu"
        className="mt-1 min-w-40 rounded-lg border border-white/10 bg-surface-dialog p-1.5 shadow-2xl"
      >
        {unstopSupported && (
          <MenuAction
            title={t("apps.unstopHelp")}
            onClick={(button) => {
              close(button);
              onAction(pkg.package, "unstop");
            }}
          >
            {t("apps.unstop")}
          </MenuAction>
        )}
        {hideSupported && (
          <MenuAction
            onClick={(button) => {
              close(button);
              onAction(pkg.package, "hide");
            }}
          >
            {t("apps.hide")}
          </MenuAction>
        )}
        {unhideSupported && (
          <MenuAction
            onClick={(button) => {
              close(button);
              onAction(pkg.package, "unhide");
            }}
          >
            {t("apps.unhide")}
          </MenuAction>
        )}
        {disableUntilUsedSupported && pkg.enabled && (
          <MenuAction
            onClick={(button) => {
              close(button);
              onAction(pkg.package, "disable_until_used");
            }}
          >
            {t("apps.disableUntilUsed")}
          </MenuAction>
        )}
        {defaultStateSupported && !pkg.enabled && (
          <MenuAction
            onClick={(button) => {
              close(button);
              onAction(pkg.package, "default_state");
            }}
          >
            {t("apps.defaultState")}
          </MenuAction>
        )}
        {suspendQuarantineSupported && pkg.enabled && (
          <MenuAction
            title={t("apps.suspendQuarantineHelp")}
            onClick={(button) => {
              close(button);
              onAction(pkg.package, "suspend_quarantine");
            }}
          >
            {t("apps.suspendQuarantine")}
          </MenuAction>
        )}
        {unsuspendSupported && pkg.enabled && (
          <MenuAction
            onClick={(button) => {
              close(button);
              onAction(pkg.package, "unsuspend");
            }}
          >
            {t("apps.unsuspend")}
          </MenuAction>
        )}
        {suspendSupported && pkg.enabled && (
          <MenuAction
            onClick={(button) => {
              close(button);
              onAction(pkg.package, "suspend");
            }}
          >
            {t("apps.suspend")}
          </MenuAction>
        )}
        {archiveSupported && !pkg.system && (
          <MenuAction
            onClick={(button) => {
              close(button);
              onAction(pkg.package, "archive");
            }}
          >
            {t("apps.archive")}
          </MenuAction>
        )}
        <MenuAction
          onClick={(button) => {
            close(button);
            onAction(pkg.package, "force_stop");
          }}
        >
          {t("apps.stop")}
        </MenuAction>
        <MenuAction
          onClick={(button) => {
            close(button);
            onInspect(pkg.package);
          }}
        >
          {t("apps.perms")}
        </MenuAction>
        <MenuAction
          onClick={(button) => {
            close(button);
            onExport(pkg.package);
          }}
        >
          {t("apps.exportApks")}
        </MenuAction>
        {showLegacyExport && (
          <MenuAction
            onClick={(button) => {
              close(button);
              onLegacyExport(pkg.package);
            }}
          >
            {t("apps.legacyData")}
          </MenuAction>
        )}
        <MenuAction
          onClick={(button) => {
            close(button);
            onAction(pkg.package, "uninstall_for_user");
          }}
          danger
        >
          {t("apps.uninstall")}
        </MenuAction>
      </div>
    </details>
  );
}

function MenuAction({
  children,
  onClick,
  danger = false,
  title,
}: {
  children: ReactNode;
  onClick: (button: HTMLButtonElement) => void;
  danger?: boolean;
  title?: string;
}) {
  return (
    <button
      type="button"
      role="menuitem"
      title={title}
      onClick={(event) => onClick(event.currentTarget)}
      className={cn(
        "block w-full rounded-sm px-3 py-2 text-start text-sm transition hover:bg-white/[0.06] focus:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-circuit-300",
        danger ? "text-red-200" : "text-anvil-200",
      )}
    >
      {children}
    </button>
  );
}

function PackageIcon({
  label,
  iconDataUri,
}: {
  label: string;
  iconDataUri: string | null | undefined;
}) {
  if (iconDataUri) {
    return (
      <img
        src={iconDataUri}
        alt=""
        className="h-9 w-9 shrink-0 rounded-lg border border-white/10 bg-anvil-900 object-contain p-1"
        loading="lazy"
      />
    );
  }
  return (
    <div
      className="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg border border-white/10 bg-circuit-300/10 text-xs font-semibold text-circuit-100"
      aria-hidden="true"
    >
      {initials(label)}
    </div>
  );
}

function packageFallbackLabel(pkg: string): string {
  return pkg.split(".").filter(Boolean).pop() ?? pkg;
}

function initials(label: string): string {
  const parts = label.trim().split(/\s+/u).filter(Boolean);
  if (parts.length >= 2) {
    return `${parts[0]?.[0] ?? ""}${parts[1]?.[0] ?? ""}`.toUpperCase();
  }
  return (parts[0] ?? "AP").slice(0, 2).toUpperCase();
}
