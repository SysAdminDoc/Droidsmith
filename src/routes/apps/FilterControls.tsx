import { useRef, type KeyboardEvent as ReactKeyboardEvent } from "react";
import { useTranslation } from "react-i18next";

import { type ActionKind, type PackageFilter } from "../../lib/tauri";
import { Button, Card } from "../common";

const FILTERS: { value: PackageFilter; labelKey: string }[] = [
  { value: "all", labelKey: "apps.filterAll" },
  { value: "user", labelKey: "apps.filterUser" },
  { value: "system", labelKey: "apps.filterSystem" },
  { value: "enabled", labelKey: "apps.filterEnabled" },
  { value: "disabled", labelKey: "apps.filterDisabled" },
  { value: "archived", labelKey: "apps.filterArchived" },
  { value: "retained", labelKey: "apps.filterRetained" },
];

/** Package-filter radiogroup with roving-tabindex keyboard navigation (IMP-67:
 *  extracted verbatim from the former Apps.tsx god-file). */
export function FilterChips({
  active,
  onChange,
}: {
  active: PackageFilter;
  onChange: (f: PackageFilter) => void;
}) {
  const { t } = useTranslation();
  const buttonsRef = useRef<(HTMLButtonElement | null)[]>([]);

  // Radiogroup keyboard contract: one tab stop (roving tabindex) plus
  // arrow/Home/End navigation that moves selection and focus together.
  const onKeyDown = (event: ReactKeyboardEvent, index: number) => {
    let next: number | null = null;
    if (event.key === "ArrowRight" || event.key === "ArrowDown") {
      next = (index + 1) % FILTERS.length;
    } else if (event.key === "ArrowLeft" || event.key === "ArrowUp") {
      next = (index - 1 + FILTERS.length) % FILTERS.length;
    } else if (event.key === "Home") {
      next = 0;
    } else if (event.key === "End") {
      next = FILTERS.length - 1;
    }
    if (next === null) return;
    event.preventDefault();
    onChange(FILTERS[next]!.value);
    buttonsRef.current[next]?.focus();
  };

  return (
    <div
      className="flex flex-wrap gap-4 border-b border-white/10"
      role="radiogroup"
      aria-label={t("apps.packageFilterLabel")}
    >
      {FILTERS.map((f, index) => (
        <button
          key={f.value}
          type="button"
          role="radio"
          aria-checked={active === f.value}
          tabIndex={active === f.value ? 0 : -1}
          ref={(element) => {
            buttonsRef.current[index] = element;
          }}
          onClick={() => onChange(f.value)}
          onKeyDown={(event) => onKeyDown(event, index)}
          className={[
            "-mb-px border-b-2 px-0.5 py-2 text-sm font-medium transition",
            "focus:outline-none focus-visible:ring-2 focus-visible:ring-circuit-300",
            active === f.value
              ? "border-circuit-300 text-circuit-100"
              : "border-transparent text-anvil-400 hover:text-anvil-100",
          ].join(" ")}
        >
          {t(f.labelKey)}
        </button>
      ))}
    </div>
  );
}

/** Multi-select batch action bar shown when packages are selected (IMP-67:
 *  extracted verbatim from the former Apps.tsx god-file). */
export function BatchActionBar({
  selectedCount,
  canDisable,
  canEnable,
  canArchive,
  canUnarchive,
  onClear,
  onAction,
}: {
  selectedCount: number;
  canDisable: boolean;
  canEnable: boolean;
  canArchive: boolean;
  canUnarchive: boolean;
  onClear: () => void;
  onAction: (kind: ActionKind) => void;
}) {
  const { t } = useTranslation();
  if (selectedCount === 0) return null;
  return (
    <Card className="sticky bottom-0 z-10 flex flex-col gap-3 border-t border-circuit-300/30 bg-anvil-950/95 px-3 py-2.5 backdrop-blur sm:flex-row sm:items-center sm:justify-between">
      <div>
        <p className="text-sm font-semibold text-anvil-50">
          {t("apps.batchSelected", { count: selectedCount })}
        </p>
        <p className="sr-only">
          {selectedCount < 2
            ? t("apps.batchSelectMore")
            : t("apps.batchReviewBody")}
        </p>
      </div>
      <div className="flex flex-wrap gap-2">
        <Button
          type="button"
          size="sm"
          onClick={() => onAction("disable")}
          disabled={!canDisable}
        >
          {t("apps.batchDisable")}
        </Button>
        <Button
          type="button"
          size="sm"
          onClick={() => onAction("enable")}
          disabled={!canEnable}
        >
          {t("apps.batchEnable")}
        </Button>
        <Button
          type="button"
          size="sm"
          onClick={() => onAction("archive")}
          disabled={!canArchive}
        >
          {t("apps.batchArchive")}
        </Button>
        <Button
          type="button"
          size="sm"
          onClick={() => onAction("request_unarchive")}
          disabled={!canUnarchive}
        >
          {t("apps.batchUnarchive")}
        </Button>
        <Button type="button" size="sm" variant="ghost" onClick={onClear}>
          {t("apps.batchClear")}
        </Button>
      </div>
    </Card>
  );
}
