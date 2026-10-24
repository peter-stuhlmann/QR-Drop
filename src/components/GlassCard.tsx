import type { ReactNode } from "react";

/** Square glass card that holds the main content of a phase */
export function GlassCard({ children }: { children: ReactNode }) {
  return <div className="glass-card relative grid size-[284px] place-items-center">{children}</div>;
}
