import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { cn } from "../lib/cn";
import { useFocusTrap } from "../lib/useFocusTrap";

export type PaletteItem = {
  id: string;
  label: string;
  description?: string;
  category: string;
};

export function CommandPalette({
  open,
  onClose,
  items,
  onSelect,
}: {
  open: boolean;
  onClose: () => void;
  items: PaletteItem[];
  onSelect: (item: PaletteItem) => void;
}) {
  const { t } = useTranslation();
  const [query, setQuery] = useState("");
  const [activeIndex, setActiveIndex] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const trapRef = useFocusTrap<HTMLDivElement>(open);

  const filtered = useMemo(() => {
    if (!query.trim()) return items;
    const lower = query.toLowerCase();
    return items.filter(
      (item) =>
        item.label.toLowerCase().includes(lower) ||
        item.id.toLowerCase().includes(lower) ||
        (item.description?.toLowerCase().includes(lower) ?? false) ||
        item.category.toLowerCase().includes(lower),
    );
  }, [items, query]);

  useEffect(() => {
    setActiveIndex(0);
  }, [filtered.length]);

  useEffect(() => {
    if (open) {
      setQuery("");
      setActiveIndex(0);
      setTimeout(() => inputRef.current?.focus(), 0);
    }
  }, [open]);

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === "Escape") {
        onClose();
      } else if (e.key === "ArrowDown") {
        e.preventDefault();
        setActiveIndex((prev) => Math.min(prev + 1, filtered.length - 1));
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        setActiveIndex((prev) => Math.max(prev - 1, 0));
      } else if (e.key === "Enter" && filtered[activeIndex]) {
        onSelect(filtered[activeIndex]!);
        onClose();
      }
    },
    [filtered, activeIndex, onClose, onSelect],
  );

  if (!open) return null;

  return (
    <div
      className="fixed inset-0 z-50 flex items-start justify-center bg-black/65 px-4 pt-[12vh] backdrop-blur-sm"
      onClick={onClose}
      role="dialog"
      aria-modal="true"
      aria-labelledby="command-palette-title"
    >
      <div
        ref={trapRef}
        tabIndex={-1}
        className="w-full max-w-xl overflow-hidden rounded-lg border border-white/10 bg-anvil-900 shadow-2xl outline-none"
        onClick={(e) => e.stopPropagation()}
        onKeyDown={handleKeyDown}
      >
        <div className="flex items-center justify-between border-b border-white/10 px-4 py-3">
          <div>
            <h2
              id="command-palette-title"
              className="text-sm font-semibold text-anvil-50"
            >
              {t("palette.title")}
            </h2>
            <p className="mt-0.5 text-xs text-anvil-500">
              {t("palette.description")}
            </p>
          </div>
          <button
            type="button"
            onClick={onClose}
            aria-label={t("palette.close")}
            className="grid h-8 w-8 place-items-center rounded-md text-anvil-400 transition hover:bg-white/[0.06] hover:text-anvil-50 focus:outline-none focus-visible:ring-2 focus-visible:ring-circuit-300"
          >
            <svg
              viewBox="0 0 24 24"
              className="h-4 w-4"
              fill="none"
              stroke="currentColor"
              strokeLinecap="round"
              strokeLinejoin="round"
              strokeWidth="2"
              aria-hidden="true"
            >
              <path d="m6 6 12 12M18 6 6 18" />
            </svg>
          </button>
        </div>
        <div className="flex items-center gap-3 border-b border-white/10 px-4 py-3">
          <svg
            viewBox="0 0 24 24"
            className="h-5 w-5 shrink-0 text-anvil-400"
            fill="none"
            stroke="currentColor"
            strokeWidth="2"
            strokeLinecap="round"
            strokeLinejoin="round"
          >
            <circle cx="11" cy="11" r="8" />
            <path d="m21 21-4.3-4.3" />
          </svg>
          <input
            ref={inputRef}
            type="text"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder={t("palette.searchPlaceholder")}
            aria-label={t("palette.searchLabel")}
            className="h-8 flex-1 bg-transparent text-sm text-anvil-50 outline-none placeholder:text-anvil-500"
          />
        </div>

        <div className="max-h-80 overflow-y-auto py-2" role="listbox">
          {filtered.length === 0 && (
            <div className="px-4 py-7 text-center">
              <p className="text-sm font-medium text-anvil-200">
                {t("palette.noMatchesTitle")}
              </p>
              <p className="mt-1 text-xs text-anvil-500">
                {t("palette.noMatchesHint")}
              </p>
            </div>
          )}
          {filtered.map((item, index) => (
            <button
              key={item.id}
              type="button"
              role="option"
              aria-selected={index === activeIndex}
              onClick={() => {
                onSelect(item);
                onClose();
              }}
              onMouseEnter={() => setActiveIndex(index)}
              className={cn(
                "flex w-full items-center gap-3 px-4 py-2.5 text-left transition-colors",
                index === activeIndex
                  ? "bg-circuit-300/10 text-anvil-50"
                  : "text-anvil-300 hover:bg-white/[0.04]",
              )}
            >
              <span className="min-w-0 flex-1">
                <span className="block text-sm font-medium">{item.label}</span>
                {item.description && (
                  <span className="mt-0.5 block text-xs text-anvil-500">
                    {item.description}
                  </span>
                )}
              </span>
              <span className="shrink-0 rounded-md border border-white/10 px-2 py-0.5 text-[10px] text-anvil-500">
                {item.category}
              </span>
            </button>
          ))}
        </div>
      </div>
    </div>
  );
}
