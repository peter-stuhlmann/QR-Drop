import { motion } from "motion/react";
import { useId, type ReactNode } from "react";

const RING_SIZE = 168;
const RING_STROKE = 9;
/** Arc length shown while the total is unknown; the ring spins instead. */
const INDETERMINATE_ARC = 0.25;

/**
 * Big transfer ring with gradient stroke. `value` is 0–1, or null while the
 * progress is unknown (indeterminate: spinning arc, no aria-valuenow).
 */
export function ProgressRing({
  value,
  label,
  valueText,
  children,
}: {
  value: number | null;
  /** Accessible name, e.g. "Übertragung". */
  label: string;
  /** Spoken value; the visible children are presentational for screen readers. */
  valueText?: string;
  children?: ReactNode;
}) {
  const id = useId();
  const r = (RING_SIZE - RING_STROKE) / 2;
  const c = 2 * Math.PI * r;
  const arc = value ?? INDETERMINATE_ARC;
  return (
    <div
      className="relative grid place-items-center"
      style={{ width: RING_SIZE, height: RING_SIZE }}
      role="progressbar"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={value === null ? undefined : Math.round(value * 100)}
      aria-valuetext={valueText}
    >
      <svg
        width={RING_SIZE}
        height={RING_SIZE}
        className={`absolute inset-0 -rotate-90 ${value === null ? "spin" : ""}`}
        aria-hidden
      >
        <defs>
          <linearGradient id={id} x1="0" y1="0" x2="1" y2="1">
            <stop offset="0" stopColor="var(--color-glow-cyan)" />
            <stop offset="1" stopColor="var(--color-accent)" />
          </linearGradient>
        </defs>
        <circle
          cx={RING_SIZE / 2}
          cy={RING_SIZE / 2}
          r={r}
          fill="none"
          stroke="var(--progress-track)"
          strokeWidth={RING_STROKE}
        />
        <motion.circle
          cx={RING_SIZE / 2}
          cy={RING_SIZE / 2}
          r={r}
          fill="none"
          stroke={`url(#${id})`}
          strokeWidth={RING_STROKE}
          strokeLinecap="round"
          strokeDasharray={c}
          initial={false}
          animate={{ strokeDashoffset: c * (1 - arc) }}
          transition={{ duration: 0.3, ease: "easeOut" }}
          style={{
            filter:
              "drop-shadow(0 0 6px color-mix(in oklab, var(--color-accent) 60%, transparent))",
          }}
        />
      </svg>
      <div className="relative flex flex-col items-center">{children}</div>
    </div>
  );
}

/** Small countdown ring next to the remaining time (20 px). */
export function CountdownRing({ fraction }: { fraction: number }) {
  const size = 20;
  const stroke = 2.2;
  const r = (size - stroke) / 2;
  const c = 2 * Math.PI * r;
  return (
    <svg width={size} height={size} className="-rotate-90" aria-hidden>
      <circle
        cx={size / 2}
        cy={size / 2}
        r={r}
        fill="none"
        stroke="var(--progress-track)"
        strokeWidth={stroke}
      />
      <circle
        cx={size / 2}
        cy={size / 2}
        r={r}
        fill="none"
        stroke="var(--color-accent)"
        strokeWidth={stroke}
        strokeLinecap="round"
        strokeDasharray={c}
        // No CSS transition: one jump per second is invisible (1/600 of the
        // ring), a running transition repaints the window 60× per second.
        strokeDashoffset={c * (1 - Math.max(0, Math.min(1, fraction)))}
      />
    </svg>
  );
}
