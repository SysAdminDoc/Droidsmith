// Devices route header actions: refresh + overflow menu (IMP-72: extracted
// verbatim from the former Devices.tsx god-file).

import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "../common";
import { MoreIcon, RecoveryIcon, RefreshIcon } from "./icons";

export function DeviceHeaderActions({
  refreshing,
  onRefresh,
  onReviewRecovery,
}: {
  refreshing: boolean;
  onRefresh: () => void;
  onReviewRecovery: () => void;
}) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const menuRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const dismiss = (event: PointerEvent) => {
      if (!menuRef.current?.contains(event.target as Node)) setOpen(false);
    };
    document.addEventListener("pointerdown", dismiss);
    return () => document.removeEventListener("pointerdown", dismiss);
  }, [open]);

  return (
    <div className="flex items-center gap-2">
      <Button
        type="button"
        onClick={onRefresh}
        disabled={refreshing}
        variant="primary"
      >
        <RefreshIcon spinning={refreshing} />
        {refreshing ? t("devices.scanning") : t("devices.refresh")}
      </Button>
      <div ref={menuRef} className="relative">
        <Button
          type="button"
          variant="secondary"
          aria-label={t("devices.moreActions")}
          aria-haspopup="menu"
          aria-expanded={open}
          className="w-10 px-0"
          onClick={() => setOpen((value) => !value)}
        >
          <MoreIcon />
        </Button>
        {open && (
          <div
            role="menu"
            className="absolute end-0 z-20 mt-2 min-w-48 rounded-lg border border-white/10 bg-surface-dialog p-1.5 shadow-2xl"
          >
            <button
              type="button"
              role="menuitem"
              className="flex w-full items-center gap-2 rounded-md px-3 py-2 text-start text-sm text-anvil-200 transition hover:bg-white/[0.07] hover:text-anvil-50 focus:outline-none focus-visible:ring-2 focus-visible:ring-circuit-300"
              onClick={() => {
                setOpen(false);
                onReviewRecovery();
              }}
            >
              <RecoveryIcon />
              {t("devices.health.reviewRecovery")}
            </button>
          </div>
        )}
      </div>
    </div>
  );
}
