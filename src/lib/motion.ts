/** Shared motion presets so every animation uses the same curve and timing. */

/** The app's one easing curve; CSS twin: `--ease-out-expo` in tokens.css. */
export const EASE_OUT_EXPO = [0.16, 1, 0.3, 1] as const;

/** Window appear: scale 0.96 → 1 with a fade in 220 ms (CLAUDE.md §7.2). */
export const APPEAR = {
  initial: { opacity: 0, scale: 0.96 },
  animate: { opacity: 1, scale: 1 },
  transition: { duration: 0.22, ease: EASE_OUT_EXPO },
} as const;

/** Crossfade between window phases (§3.3): appears like a window, leaves with a slight shrink. */
export const CROSSFADE = {
  ...APPEAR,
  exit: { opacity: 0, scale: 0.98 },
} as const;
