/** Small React hooks shared by the windows. */
import { useEffect, useState } from "react";

/** Selector of an open modal dialog (see NetworkPicker). */
const OPEN_MODAL = '[aria-modal="true"]';

/**
 * Calls `onEscape` when Esc is pressed anywhere in the window – unless a
 * modal dialog is open or a nested handler already took the key: Esc inside
 * the network picker must only close the picker, never cancel the share.
 */
export function useEscape(onEscape: () => void): void {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || e.defaultPrevented) return;
      if (document.querySelector(OPEN_MODAL)) return;
      onEscape();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onEscape]);
}

/**
 * Current time in ms, refreshed every `intervalMs`. Pass `null` to stop the
 * timer while nothing on screen depends on the clock (saves re-renders).
 */
export function useNow(intervalMs: number | null): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (intervalMs === null) return;
    const timer = setInterval(() => setNow(Date.now()), intervalMs);
    return () => clearInterval(timer);
  }, [intervalMs]);
  return now;
}
