import { ChevronRight, QrCode } from "lucide-react";
import { motion, useReducedMotion } from "motion/react";
import { Button } from "../components/Button";
import { GlassWindow } from "../components/GlassWindow";
import { HintBanner } from "../components/HintBanner";
import { TitleBar } from "../components/TitleBar";
import { t } from "../i18n";
import { EASE_OUT_EXPO } from "../lib/motion";
import type { Os } from "../lib/platform";

function MenuRow({
  label,
  chevron,
  highlight,
}: {
  label: string;
  chevron?: boolean;
  highlight?: boolean;
}) {
  return (
    <div
      className={`flex items-center justify-between rounded-md px-2.5 py-1.5 text-caption ${highlight ? "font-semibold text-white" : ""}`}
      style={
        highlight
          ? {
              background:
                "linear-gradient(90deg, var(--color-accent-from), var(--color-accent-to))",
            }
          : chevron
            ? { background: "var(--surface-control)" }
            : undefined
      }
    >
      <span className="flex items-center gap-2">
        {highlight ? <QrCode size={15} color="white" /> : null}
        {label}
      </span>
      {chevron ? <ChevronRight size={14} color="var(--text-secondary)" /> : null}
    </div>
  );
}

/** First start on macOS: explains where the Quick Action lives. */
export function WelcomeView({
  os,
  onClose,
  onDone,
}: {
  os: Os;
  onClose: () => void;
  onDone: () => void;
}) {
  const reduce = useReducedMotion();
  return (
    <GlassWindow>
      <TitleBar os={os} onClose={onClose} />
      <div className="mt-1 flex flex-col items-center gap-1 text-center">
        <div className="accent-tile grid size-14 place-items-center rounded-[15px]">
          <QrCode size={28} color="white" />
        </div>
        <h1 className="mt-2 text-[22px] font-semibold">{t("welcome.title")}</h1>
        <p className="text-caption text-[var(--text-secondary)]">{t("welcome.line1")}</p>
        <p className="text-caption text-[var(--text-secondary)]">{t("welcome.line2")}</p>
      </div>

      <div
        className="glass-card mt-3 flex flex-col gap-0.5 p-1.5"
        style={{ borderRadius: 12 }}
        aria-hidden
      >
        <MenuRow label={t("welcome.menu.open")} />
        <MenuRow label={t("welcome.menu.info")} />
        <div className="mx-2 my-1 h-px" style={{ background: "var(--border-control)" }} />
        <MenuRow label={t("welcome.menu.quick")} chevron />
        <motion.div
          initial={reduce ? false : { opacity: 0, x: -8 }}
          animate={{ opacity: 1, x: 0 }}
          transition={{ delay: 0.6, duration: 0.4, ease: EASE_OUT_EXPO }}
        >
          <motion.div
            animate={reduce ? undefined : { scale: [1, 1.02, 1] }}
            transition={{ delay: 1.2, duration: 1.6, repeat: Infinity, ease: "easeInOut" }}
          >
            <MenuRow label={t("welcome.menu.send")} highlight />
          </motion.div>
        </motion.div>
      </div>
      <p className="mt-1.5 text-center text-[12px] text-balance text-[var(--text-secondary)]">
        {t("welcome.text")}
      </p>

      <div className="mt-2">
        <HintBanner tone="info">{t("welcome.firewall")}</HintBanner>
      </div>
      <div className="mt-auto pt-2">
        <Button variant="primary" large block onClick={onDone}>
          {t("welcome.go")}
        </Button>
      </div>
    </GlassWindow>
  );
}
