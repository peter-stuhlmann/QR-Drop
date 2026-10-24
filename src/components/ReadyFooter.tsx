import { AnimatePresence } from "motion/react";
import { useState } from "react";
import { t } from "../i18n";
import { formatClock } from "../lib/format";
import type { NetworkOption } from "../lib/tauri";
import { CancelButton } from "./ActionButtons";
import { NetworkPicker } from "./NetworkPicker";
import { CountdownRing } from "./ProgressRing";

/** Validity of a code when the backend has not said yet (CLAUDE.md §5.2). */
const DEFAULT_TTL_MS = 10 * 60_000;

/**
 * Footer while the QR code waits for a scan: countdown, "Anderes Netzwerk?"
 * (with its picker) when there is more than one LAN, and Cancel.
 */
export function ReadyFooter({
  remainingMs,
  ttlMs = DEFAULT_TTL_MS,
  networks,
  selectedIp,
  onSelectNetwork,
  onCancel,
}: {
  remainingMs: number;
  ttlMs?: number;
  networks: NetworkOption[];
  selectedIp: string | null;
  onSelectNetwork: (ip: string) => void;
  onCancel: () => void;
}) {
  const [pickerOpen, setPickerOpen] = useState(false);
  const clock = formatClock(remainingMs);
  return (
    <div className="flex w-full items-center justify-between">
      {/* role="timer" never announces by itself; the label gives the full sentence. */}
      <span
        role="timer"
        aria-label={t("share.validFor", { time: clock })}
        className="tabular flex items-center gap-2 text-caption font-medium text-[var(--text-secondary)]"
      >
        <CountdownRing fraction={remainingMs / ttlMs} />
        {clock}
      </span>
      {networks.length > 1 ? (
        <button
          type="button"
          aria-haspopup="dialog"
          className="focus-ring rounded text-caption text-[var(--text-link)] hover:underline"
          onClick={() => setPickerOpen(true)}
        >
          {t("share.otherNetwork")}
        </button>
      ) : null}
      <CancelButton onClick={onCancel} />
      <AnimatePresence>
        {pickerOpen ? (
          <NetworkPicker
            networks={networks}
            selected={selectedIp}
            onSelect={(ip) => {
              setPickerOpen(false);
              onSelectNetwork(ip);
            }}
            onClose={() => setPickerOpen(false)}
          />
        ) : null}
      </AnimatePresence>
    </div>
  );
}
