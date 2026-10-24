import { Cable, Check, Network, Wifi } from "lucide-react";
import { motion } from "motion/react";
import { useEffect, useId, useRef, type KeyboardEvent } from "react";
import { t } from "../i18n";
import { EASE_OUT_EXPO } from "../lib/motion";
import type { NetworkOption } from "../lib/tauri";

const ICON = { wifi: Wifi, ethernet: Cable, other: Network };
const LABEL = {
  wifi: "network.wifi",
  ethernet: "network.ethernet",
  other: "network.other",
} as const;

/** Keeps Tab / Shift+Tab inside `container` by wrapping at both ends. */
function trapTab(e: KeyboardEvent, container: HTMLElement) {
  const buttons = container.querySelectorAll<HTMLButtonElement>("button");
  const first = buttons[0];
  const last = buttons[buttons.length - 1];
  if (!first || !last) return;
  if (e.shiftKey && document.activeElement === first) {
    e.preventDefault();
    last.focus();
  } else if (!e.shiftKey && document.activeElement === last) {
    e.preventDefault();
    first.focus();
  }
}

/**
 * "Anderes Netzwerk?" -- modal sheet to choose the LAN address for the QR
 * code. Focus starts on the current network, stays inside while open and
 * returns to the opener afterwards; Esc and a click outside close it.
 */
export function NetworkPicker({
  networks,
  selected,
  onSelect,
  onClose,
}: {
  networks: NetworkOption[];
  selected: string | null;
  onSelect: (ip: string) => void;
  onClose: () => void;
}) {
  const dialog = useRef<HTMLDivElement>(null);
  const titleId = useId();

  useEffect(() => {
    const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const current = dialog.current?.querySelector<HTMLButtonElement>('[aria-current="true"]');
    (current ?? dialog.current?.querySelector("button"))?.focus();
    return () => {
      if (opener?.isConnected) opener.focus();
    };
  }, []);

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.key === "Escape") {
      // Only the picker closes; the window's Esc (cancel share) must not fire.
      e.preventDefault();
      e.stopPropagation();
      onClose();
    } else if (e.key === "Tab" && dialog.current) {
      trapTab(e, dialog.current);
    }
  };

  return (
    <motion.div
      className="absolute inset-0 z-20 flex items-end p-3"
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      exit={{ opacity: 0 }}
      onClick={onClose}
      style={{ background: "var(--surface-scrim)" }}
    >
      <motion.div
        ref={dialog}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        // Focusable itself, so a click on its text keeps focus inside the dialog.
        tabIndex={-1}
        className="glass-card w-full p-2 outline-none"
        style={{ background: "var(--surface-window-solid)", borderRadius: "var(--radius-card)" }}
        initial={{ y: 24, opacity: 0 }}
        animate={{ y: 0, opacity: 1 }}
        exit={{ y: 24, opacity: 0 }}
        transition={{ duration: 0.25, ease: EASE_OUT_EXPO }}
        onClick={(e) => e.stopPropagation()}
        onKeyDown={onKeyDown}
      >
        <p id={titleId} className="px-2 pt-1 pb-0.5 text-caption font-semibold">
          {t("share.chooseNetwork")}
        </p>
        <p className="px-2 pb-2 text-[12px] text-[var(--text-secondary)]">
          {t("share.networkHint")}
        </p>
        {networks.map((n) => {
          const Icon = ICON[n.kind];
          const active = n.ip === selected;
          return (
            <button
              key={n.ip}
              type="button"
              aria-current={active ? "true" : undefined}
              onClick={() => onSelect(n.ip)}
              className="focus-ring flex w-full items-center gap-3 rounded-lg px-2 py-2 text-left hover:bg-[var(--surface-control)]"
            >
              <Icon size={18} color="var(--icon-default)" />
              <span className="flex-1">
                <span className="block text-caption font-medium">{t(LABEL[n.kind])}</span>
                <span className="tabular block text-[12px] text-[var(--text-secondary)]">
                  {n.name} · {n.ip}
                </span>
              </span>
              {active && <Check size={16} color="var(--color-accent)" />}
            </button>
          );
        })}
      </motion.div>
    </motion.div>
  );
}
