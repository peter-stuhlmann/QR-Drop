import { motion, useReducedMotion } from "motion/react";
import type { ReactNode } from "react";
import { EASE_OUT_EXPO } from "../lib/motion";
import type { Tone } from "../state/session";

/** Gradient stops of the inner disc per tone. */
const TONES: Record<Tone, [string, string]> = {
  accent: ["var(--color-accent)", "var(--color-glow-magenta)"],
  success: ["var(--color-success)", "var(--color-glow-teal)"],
  warning: ["var(--color-warning)", "var(--color-glow-orange)"],
  danger: ["var(--color-danger)", "var(--color-glow-pink)"],
};

/** Round icon with soft pulse rings; decorative, the card title says it. */
export function StatusIcon({ tone, children }: { tone: Tone; children: ReactNode }) {
  const [from, to] = TONES[tone];
  const reduce = useReducedMotion();
  return (
    <div className="relative grid size-[120px] place-items-center" aria-hidden>
      <motion.div
        className="absolute inset-0 rounded-full"
        style={{
          background: `color-mix(in oklab, ${from} 10%, transparent)`,
          boxShadow: `inset 0 0 0 1px color-mix(in oklab, ${from} 25%, transparent)`,
        }}
        animate={reduce ? undefined : { scale: [1, 1.06, 1], opacity: [1, 0.7, 1] }}
        transition={{ duration: 2.4, repeat: Infinity, ease: "easeInOut" }}
      />
      <div
        className="absolute inset-3 rounded-full"
        style={{ background: `color-mix(in oklab, ${from} 16%, transparent)` }}
      />
      <div
        className="relative grid size-[72px] place-items-center rounded-full"
        style={{
          background: `linear-gradient(135deg, ${from}, ${to})`,
          boxShadow: `inset 0 1px 0 rgb(255 255 255 / 0.4), 0 8px 24px color-mix(in oklab, ${from} 55%, transparent)`,
        }}
      >
        {children}
      </div>
    </div>
  );
}

/** Check mark that draws itself (SVG stroke-draw). */
export function AnimatedCheck() {
  const reduce = useReducedMotion();
  return (
    <svg viewBox="0 0 24 24" className="size-[34px]" fill="none">
      <motion.path
        d="M20 6 9 17l-5-5"
        stroke="white"
        strokeWidth={2.6}
        strokeLinecap="round"
        strokeLinejoin="round"
        initial={{ pathLength: reduce ? 1 : 0 }}
        animate={{ pathLength: 1 }}
        transition={{ duration: 0.45, delay: 0.15, ease: EASE_OUT_EXPO }}
      />
    </svg>
  );
}
