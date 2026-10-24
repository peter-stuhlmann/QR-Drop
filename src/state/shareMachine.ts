/**
 * State machine of the share window (CLAUDE.md §3.3):
 * preparing → ready → connected → transferring → done
 * plus expired, cancelled and error.
 */
import type { ErrorReason } from "../lib/tauri";
import type { SessionAction } from "./session";

export type SharePhase =
  | { phase: "preparing" }
  | { phase: "ready" }
  | { phase: "connected"; interrupted: boolean }
  | { phase: "transferring"; sent: number; total: number; bytesPerSec: number }
  | { phase: "done" }
  | { phase: "expired" }
  | { phase: "cancelled" }
  | { phase: "error"; reason: ErrorReason };

export type ShareAction =
  | SessionAction
  | { type: "progress"; sent: number; total: number; bytesPerSec: number }
  | { type: "done" };

export const initialState: SharePhase = { phase: "preparing" };

const isFinal = (s: SharePhase) => s.phase === "done" || s.phase === "cancelled";

export function shareReducer(state: SharePhase, action: ShareAction): SharePhase {
  if (action.type === "cancel") return { phase: "cancelled" };
  if (isFinal(state)) return state;

  switch (action.type) {
    case "info": {
      const { info } = action;
      switch (info.status) {
        case "preparing":
          return { phase: "preparing" };
        case "expired":
          return { phase: "expired" };
        case "error":
          return { phase: "error", reason: info.error ?? "generic" };
        case "ready":
          // A refreshed QR (e.g. other network) must not reset a running transfer.
          if (state.phase === "connected" || state.phase === "transferring") return state;
          return { phase: "ready" };
      }
      return state;
    }
    case "connected":
      if (state.phase === "ready" || state.phase === "preparing") {
        return { phase: "connected", interrupted: false };
      }
      return state;
    case "progress":
      if (state.phase === "expired" || state.phase === "error") return state;
      return {
        phase: "transferring",
        sent: action.sent,
        total: action.total,
        bytesPerSec: action.bytesPerSec,
      };
    case "interrupted":
      if (state.phase === "transferring" || state.phase === "connected") {
        return { phase: "connected", interrupted: true };
      }
      return state;
    case "done":
      return { phase: "done" };
    case "expired":
      // A running transfer finishes even if the token expires meanwhile.
      if (state.phase === "transferring") return state;
      return { phase: "expired" };
    case "failed":
      return { phase: "error", reason: action.reason };
  }
}

/** 0–1 progress for the ring, null if unknown. */
export function progressFraction(state: SharePhase): number | null {
  if (state.phase !== "transferring" || state.total <= 0) return null;
  return Math.min(1, state.sent / state.total);
}

/** Remaining seconds or null while the speed is unknown. */
export function etaSeconds(state: SharePhase): number | null {
  if (state.phase !== "transferring" || state.bytesPerSec <= 0) return null;
  return Math.max(0, state.total - state.sent) / state.bytesPerSec;
}
