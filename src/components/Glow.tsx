import type { Tone } from "../state/session";

const GRADIENTS: Record<Tone, string> = {
  accent:
    "conic-gradient(from 0deg, var(--color-accent), var(--color-glow-magenta), var(--color-glow-cyan), var(--color-accent))",
  success:
    "conic-gradient(from 0deg, var(--color-success), var(--color-glow-cyan), var(--color-success))",
  warning:
    "conic-gradient(from 0deg, var(--color-warning), var(--color-danger), var(--color-warning))",
  danger:
    "conic-gradient(from 0deg, var(--color-danger), var(--color-glow-magenta), var(--color-danger))",
};

/** Slowly rotating colour glow behind the main card (very subtle, §7.2). */
export function Glow({ tone }: { tone: Tone }) {
  return (
    <div
      aria-hidden
      className="glow-spin pointer-events-none absolute top-1/2 left-1/2 -mt-[150px] -ml-[150px] size-[300px] rounded-full"
      style={{ background: GRADIENTS[tone], filter: "blur(48px)", opacity: "var(--glow-opacity)" }}
    />
  );
}
