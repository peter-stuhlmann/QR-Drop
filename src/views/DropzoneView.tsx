import { ArrowLeft, ClipboardPaste, MonitorDown, Sparkles, Upload } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import { useState } from "react";
import { Button } from "../components/Button";
import { GlassWindow } from "../components/GlassWindow";
import { TitleBar } from "../components/TitleBar";
import { t, type MessageKey } from "../i18n";
import { EASE_OUT_EXPO } from "../lib/motion";
import type { Os } from "../lib/platform";

const HELP: [MessageKey, MessageKey][] = [
  ["help.q1", "help.a1"],
  ["help.q2", "help.a2"],
  ["help.q3", "help.a3"],
  ["help.q4", "help.a4"],
  ["help.q5", "help.a5"],
];

/** State of the update hint at the bottom of the drop zone. */
export type UpdateState =
  | { phase: "available"; version: string }
  | { phase: "downloading"; percent: number | null }
  | { phase: "failed"; busy?: boolean };

function UpdateHint({ update, onUpdate }: { update: UpdateState; onUpdate: () => void }) {
  if (update.phase === "available") {
    return (
      <button
        type="button"
        className="focus-ring flex items-center gap-1.5 rounded font-medium text-[var(--text-link)] hover:underline"
        onClick={onUpdate}
      >
        <Sparkles size={13} />
        {t("update.available", { version: update.version })}
      </button>
    );
  }
  return (
    <p className="tabular text-[var(--text-secondary)]" aria-live="polite">
      {update.phase === "failed"
        ? t(update.busy ? "update.busy" : "update.failed")
        : update.percent === null
          ? t("update.installing")
          : t("update.downloading", { percent: update.percent })}
    </p>
  );
}

/** Started without a file: drag & drop zone plus help (CLAUDE.md §4.1). */
export function DropzoneView({
  os,
  dragging,
  onClose,
  onPick,
  onClipboard,
  onReceive,
  update = null,
  onUpdate = () => {},
}: {
  os: Os;
  dragging: boolean;
  onClose: () => void;
  onPick: () => void;
  onClipboard: () => void;
  onReceive: () => void;
  update?: UpdateState | null;
  onUpdate?: () => void;
}) {
  const [help, setHelp] = useState(false);
  return (
    <GlassWindow>
      <TitleBar os={os} onClose={onClose} />
      <AnimatePresence mode="wait" initial={false}>
        {help ? (
          <motion.div
            key="help"
            className="mt-2 flex min-h-0 flex-1 flex-col"
            initial={{ opacity: 0, x: 12 }}
            animate={{ opacity: 1, x: 0 }}
            exit={{ opacity: 0, x: 12 }}
          >
            <h1 className="text-title font-semibold">{t("help.title")}</h1>
            <div className="mt-3 flex flex-1 flex-col gap-3 overflow-auto pr-1">
              {HELP.map(([q, a]) => (
                <div key={q}>
                  <p className="text-caption font-semibold">{t(q)}</p>
                  <p className="mt-0.5 text-caption leading-snug text-[var(--text-secondary)]">
                    {t(a)}
                  </p>
                </div>
              ))}
            </div>
            <div className="pt-3">
              <Button icon={<ArrowLeft size={14} />} onClick={() => setHelp(false)}>
                {t("help.back")}
              </Button>
            </div>
          </motion.div>
        ) : (
          <motion.div
            key="drop"
            className="flex min-h-0 flex-1 flex-col"
            initial={{ opacity: 0, x: -12 }}
            animate={{ opacity: 1, x: 0 }}
            exit={{ opacity: 0, x: -12 }}
          >
            <div className="mt-2 text-center">
              <h1 className="text-title font-semibold">{t("drop.title")}</h1>
              <p className="mt-0.5 text-caption text-[var(--text-secondary)]">
                {t("drop.subtitle")}
              </p>
            </div>
            <motion.div
              className="mt-3.5 flex flex-1 flex-col items-center justify-center gap-1.5 rounded-2xl text-center"
              animate={{ scale: dragging ? 1.02 : 1 }}
              transition={{ duration: 0.2, ease: EASE_OUT_EXPO }}
              style={{
                background: dragging
                  ? "color-mix(in oklab, var(--color-accent) 18%, transparent)"
                  : "color-mix(in oklab, var(--color-accent) 7%, transparent)",
                border: "1.5px dashed color-mix(in oklab, var(--color-accent) 60%, transparent)",
              }}
            >
              <div className="accent-tile grid size-[72px] place-items-center rounded-full">
                <Upload size={30} color="white" />
              </div>
              <p className="mt-2.5 text-[17px] font-semibold">
                {dragging ? t("drop.release") : t("drop.zone")}
              </p>
              <p className="text-caption text-[var(--text-secondary)]">{t("drop.zoneHint")}</p>
              <div className="mt-2 flex flex-wrap justify-center gap-2">
                <Button onClick={onPick}>{t("drop.pick")}</Button>
                <Button icon={<ClipboardPaste size={14} />} onClick={onClipboard}>
                  {t("drop.clipboard")}
                </Button>
              </div>
              <p className="mt-1 text-[12px] text-[var(--text-tertiary)]">
                {t("drop.pasteHint", { keys: os === "macos" ? "⌘V" : t("keys.paste") })}
              </p>
            </motion.div>
            <Button
              className="mt-3"
              block
              large
              icon={<MonitorDown size={16} />}
              onClick={onReceive}
            >
              {t("drop.receive")}
            </Button>
            <div className="mt-3 flex flex-col items-center gap-1 text-center text-[12px]">
              {update ? (
                <UpdateHint update={update} onUpdate={onUpdate} />
              ) : (
                <p className="text-[var(--text-secondary)]">
                  {os === "macos" ? t("drop.tipMac") : t("drop.tipWindows")}
                </p>
              )}
              <button
                type="button"
                className="focus-ring rounded font-medium text-[var(--text-link)] hover:underline"
                onClick={() => setHelp(true)}
              >
                {t("drop.help")}
              </button>
            </div>
          </motion.div>
        )}
      </AnimatePresence>
    </GlassWindow>
  );
}
