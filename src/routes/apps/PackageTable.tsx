import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
} from "react";
import { useTranslation } from "react-i18next";

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
  const allVisibleSelected =
    packages.length > 0 &&
    packages.every((pkg) => selectedPackages.has(pkg.package));
  const gridColumnCount = 5;
  const { gridRef, onKeyDown, cellProps } = useRovingGrid(
    packages.length + 1,
    gridColumnCount,
  );

  return (
    <Card className="overflow-hidden p-0">
      <div className="flex flex-col gap-3 border-b border-white/10 p-4 sm:flex-row sm:items-center sm:justify-between">
        <div>
          <h3 className="text-sm font-semibold text-anvil-50">
            {t("apps.installedPackages")}
          </h3>
          <p className="mt-1 text-xs text-anvil-400">
            {t("apps.installedPackagesBody")}
          </p>
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
        <div className="overflow-x-auto">
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
                <TableHeaderCell {...cellProps(0, 1)}>
                  {t("apps.package")}
                </TableHeaderCell>
                <TableHeaderCell {...cellProps(0, 2)}>
                  {t("apps.type")}
                </TableHeaderCell>
                <TableHeaderCell {...cellProps(0, 3)}>
                  {t("devices.state")}
                </TableHeaderCell>
                <TableHeaderCell {...cellProps(0, 4)}>
                  {t("apps.actions")}
                </TableHeaderCell>
              </tr>
            </thead>
            <tbody className="divide-y divide-white/10">
              {packages.map((pkg, rowIndex) => (
                <tr
                  key={pkg.package}
                  role="row"
                  aria-rowindex={rowIndex + 2}
                  aria-selected={selectedPackages.has(pkg.package)}
                  className="bg-anvil-950/20 transition hover:bg-white/[0.035]"
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
                    <div className="flex min-w-[10rem] flex-wrap gap-1.5">
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
                        <>
                          <Button
                            type="button"
                            size="sm"
                            onClick={() => onAction(pkg.package, "disable")}
                          >
                            {t("apps.disable")}
                          </Button>
                          <Button
                            type="button"
                            size="sm"
                            onClick={() =>
                              onAction(pkg.package, "uninstall_for_user")
                            }
                            variant="danger"
                          >
                            {t("apps.uninstall")}
                          </Button>
                        </>
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
                      {!pkg.archived &&
                        !pkg.retained &&
                        archiveSupported &&
                        !pkg.system && (
                          <Button
                            type="button"
                            size="sm"
                            onClick={() => onAction(pkg.package, "archive")}
                          >
                            {t("apps.archive")}
                          </Button>
                        )}
                      {!pkg.archived && !pkg.retained && (
                        <>
                          <Button
                            type="button"
                            size="sm"
                            variant="ghost"
                            onClick={() => onAction(pkg.package, "force_stop")}
                          >
                            {t("apps.stop")}
                          </Button>
                          <Button
                            type="button"
                            size="sm"
                            variant="ghost"
                            onClick={() => onInspect(pkg.package)}
                          >
                            {t("apps.perms")}
                          </Button>
                          <Button
                            type="button"
                            size="sm"
                            variant="ghost"
                            onClick={() => onExport(pkg.package)}
                          >
                            {t("apps.exportApks")}
                          </Button>
                        </>
                      )}
                      {!pkg.archived && !pkg.retained && showLegacyExport && (
                        <Button
                          type="button"
                          size="sm"
                          variant="ghost"
                          onClick={() => onLegacyExport(pkg.package)}
                        >
                          {t("apps.legacyData")}
                        </Button>
                      )}
                    </div>
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
          <p className="mt-1 text-[11px] text-anvil-500">
            {t("apps.viaInstaller", { installer: pkg.installer })}
          </p>
        )}
      </div>
    </div>
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
