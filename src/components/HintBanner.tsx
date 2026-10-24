import { ShieldCheck, TriangleAlert } from "lucide-react";
import { motion } from "motion/react";
import type { ReactNode } from "react";
import { EASE_OUT_EXPO } from "../lib/motion";

/**
 * Subtle banner, e.g. the firewall hint after 60 s without a request. No
 * live region of its own: windows announce through PhaseStage, and a
 * nested role="status" would be read twice.
 */
export function HintBanner({
  tone = "warning",
  children,
}: {
  tone?: "warning" | "info";
  children: ReactNode;
}) {
  const color = tone === "warning" ? "var(--color-warning)" : "var(--color-accent)";
  const Icon = tone === "warning" ? TriangleAlert : ShieldCheck;
  return (
    <motion.div
      initial={{ opacity: 0, y: 6 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.3, ease: EASE_OUT_EXPO }}
      className="flex w-full items-center gap-2.5 rounded-xl px-3 py-2.5 text-left text-[12px] leading-snug text-[var(--text-primary)]"
      style={{
        background: `color-mix(in oklab, ${color} 12%, transparent)`,
        boxShadow: `inset 0 0 0 1px color-mix(in oklab, ${color} 30%, transparent)`,
      }}
    >
      <Icon size={18} color={color} className="shrink-0" />
      <span>{children}</span>
    </motion.div>
  );
}
