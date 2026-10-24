/**
 * State machine of the receive window (phone → computer):
 * preparing → ready → connected → uploading → received
 * plus expired, cancelled and error. Unlike a share it never ends on its
 * own: the phone can send more until the window is closed.
 */
import type { ErrorReason, FileKind } from "../lib/tauri";
import type { SessionAction } from "./session";

export interface ReceivedItem {
  /** Stable React key. */
  key: string;
  kind: FileKind;
  /** File name; for texts the first line (or nothing for passwords). */
  name: string;
  /** Bytes for files, characters for texts. */
  size: number;
}

export type ReceivePhase =
  | { phase: "preparing" }
  | { phase: "ready" }
  | { phase: "connected"; interrupted: boolean }
  | {
      phase: "uploading";
      name: string;
      index: number;
      count: number;
      sent: number;
      total: number;
      bytesPerSec: number;
    }
  | { phase: "received"; interrupted: boolean }
  | { phase: "expired" }
  | { phase: "cancelled" }
  | { phase: "error"; reason: ErrorReason };

export interface ReceiveState {
  view: ReceivePhase;
  /** Everything that arrived in this window, oldest first. */
  items: ReceivedItem[];
}

export type ReceiveAction =
  | SessionAction
  | {
      type: "upload";
      name: string;
      index: number;
      count: number;
      sent: number;
      total: number;
      bytesPerSec: number;
    }
  | { type: "received"; item: ReceivedItem };

export const initialReceiveState: ReceiveState = { view: { phase: "preparing" }, items: [] };

const active = (v: ReceivePhase) =>
  v.phase === "connected" || v.phase === "uploading" || v.phase === "received";

function withView(state: ReceiveState, view: ReceivePhase): ReceiveState {
  return view === state.view ? state : { ...state, view };
}

export function receiveReducer(state: ReceiveState, action: ReceiveAction): ReceiveState {
  const v = state.view;
  if (action.type === "cancel") return withView(state, { phase: "cancelled" });
  if (v.phase === "cancelled") return state;

  switch (action.type) {
    case "info":
      switch (action.info.status) {
        case "preparing":
          return withView(state, { phase: "preparing" });
        case "expired":
          return withView(state, { phase: "expired" });
        case "error":
          return withView(state, { phase: "error", reason: action.info.error ?? "generic" });
        case "ready":
          // A refreshed QR code (other network) must not hide a running
          // transfer or what arrived; after a new code the QR shows again.
          if (active(v)) return state;
          return withView(state, { phase: "ready" });
      }
      return state;
    case "connected":
      if (v.phase === "ready" || v.phase === "preparing") {
        return withView(state, { phase: "connected", interrupted: false });
      }
      return state;
    case "upload":
      if (v.phase === "expired" || v.phase === "error") return state;
      return withView(state, {
        phase: "uploading",
        name: action.name,
        index: action.index,
        count: action.count,
        sent: action.sent,
        total: action.total,
        bytesPerSec: action.bytesPerSec,
      });
    case "received": {
      // Keys are unique per arrival (ReceiveWindow counts them), so a known
      // key means the same arrival was dispatched twice: show it only once.
      if (state.items.some((i) => i.key === action.item.key)) return state;
      const items = [...state.items, action.item];
      // More files of the same batch follow: keep showing the progress.
      const more = v.phase === "uploading" && v.index < v.count;
      return {
        items,
        view: more ? v : { phase: "received", interrupted: false },
      };
    }
    case "interrupted":
      if (v.phase !== "uploading" && v.phase !== "connected") return state;
      return withView(
        state,
        state.items.length
          ? { phase: "received", interrupted: true }
          : { phase: "connected", interrupted: true },
      );
    case "expired":
      // A running upload finishes even if the code expires meanwhile.
      if (v.phase === "uploading") return state;
      return withView(state, { phase: "expired" });
    case "failed":
      return withView(state, { phase: "error", reason: action.reason });
  }
}

/** 0–1 progress of the whole batch, null if unknown. */
export function uploadFraction(v: ReceivePhase): number | null {
  if (v.phase !== "uploading" || v.total <= 0) return null;
  return Math.min(1, v.sent / v.total);
}

/** Remaining seconds of the batch or null while the speed is unknown. */
export function uploadEta(v: ReceivePhase): number | null {
  if (v.phase !== "uploading" || v.bytesPerSec <= 0) return null;
  return Math.max(0, v.total - v.sent) / v.bytesPerSec;
}

/** Counts for the summary line: "3 Dateien · 1 Text". */
export function receivedCounts(items: ReceivedItem[]): { files: number; texts: number } {
  const texts = items.filter((i) => i.kind === "text" || i.kind === "secret").length;
  return { files: items.length - texts, texts };
}
