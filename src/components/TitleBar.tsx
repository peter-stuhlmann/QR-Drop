import { X } from "lucide-react";
import { t } from "../i18n";
import type { Os } from "../lib/platform";

function WindowCloseButton({ onClose }: { onClose: () => void }) {
  return (
    <button
      type="button"
      onClick={onClose}
      aria-label={t("a11y.close")}
      className="focus-ring glass-control grid size-7 place-items-center rounded-full transition-colors"
    >
      <X size={14} strokeWidth={2.4} color="var(--icon-default)" />
    </button>
  );
}

/**
 * Invisible drag region with a round close button: left on macOS, right on
 * Windows (CLAUDE.md §7.1).
 */
export function TitleBar({ os, onClose }: { os: Os; onClose: () => void }) {
  const mac = os === "macos";
  return (
    <div data-tauri-drag-region className="flex h-7 shrink-0 items-center justify-between">
      {mac ? <WindowCloseButton onClose={onClose} /> : <span className="size-7" />}
      <span
        data-tauri-drag-region
        className="text-caption font-semibold text-[var(--text-tertiary)]"
      >
        {t("app.name")}
      </span>
      {mac ? <span className="size-7" /> : <WindowCloseButton onClose={onClose} />}
    </div>
  );
}
