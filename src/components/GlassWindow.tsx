import { motion } from "motion/react";
import type { ReactNode } from "react";
import { APPEAR } from "../lib/motion";

/** Window surface with the appear animation (scale 0.96 → 1, 220 ms). */
export function GlassWindow({ children }: { children: ReactNode }) {
  return (
    <motion.div className="glass-window flex flex-col px-5 pt-3.5 pb-5" {...APPEAR}>
      {children}
    </motion.div>
  );
}
