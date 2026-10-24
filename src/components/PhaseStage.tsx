import { AnimatePresence, motion } from "motion/react";
import type { ReactNode } from "react";
import { CROSSFADE } from "../lib/motion";
import type { PhaseName, Tone } from "../state/session";
import { Glow } from "./Glow";

/**
 * The middle of a share/receive window: the phase's card crossfades in front
 * of the glow (§3.3). Also hosts the window's one live region, which stays
 * mounted across phases so screen readers announce each new status.
 */
export function PhaseStage({
  phase,
  tone,
  announcement,
  children,
}: {
  /** Changing it crossfades to the new card. */
  phase: PhaseName;
  tone: Tone;
  /** Spoken status, e.g. the card title; not every hint (a per-second countdown would chatter). */
  announcement?: string;
  children: ReactNode;
}) {
  return (
    <div className="relative grid min-h-0 flex-1 place-items-center">
      <Glow tone={tone} />
      <AnimatePresence mode="wait" initial={false}>
        <motion.div key={phase} {...CROSSFADE}>
          {children}
        </motion.div>
      </AnimatePresence>
      <p className="sr-only" aria-live="polite" aria-atomic="true">
        {announcement}
      </p>
    </div>
  );
}
