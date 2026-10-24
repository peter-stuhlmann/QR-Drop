/**
 * What share and receive windows have in common: the backend actions both
 * state machines understand and how a phase maps to colour and closing.
 */
import type { ErrorReason, ShareInfo } from "../lib/tauri";
import type { ReceivePhase } from "./receiveMachine";
import type { SharePhase } from "./shareMachine";

/** Actions dispatched by useShareSession; both reducers accept all of them. */
export type SessionAction =
  | { type: "info"; info: ShareInfo }
  | { type: "connected" }
  | { type: "interrupted" }
  | { type: "expired" }
  | { type: "cancel" }
  | { type: "failed"; reason: ErrorReason };

export type PhaseName = SharePhase["phase"] | ReceivePhase["phase"];

/** Colour family of status icons and the glow behind the card. */
export type Tone = "accent" | "success" | "warning" | "danger";

/** Success, warning or danger for the end states; accent while working. */
export function toneOf(phase: PhaseName): Tone {
  switch (phase) {
    case "done":
    case "received":
      return "success";
    case "expired":
      return "warning";
    case "error":
      return "danger";
    default:
      return "accent";
  }
}

/**
 * True when closing the window loses nothing (transfer finished, code
 * expired, error): then Esc and the title-bar button close instead of cancel.
 */
export function isSettled(phase: PhaseName): boolean {
  return phase === "done" || phase === "received" || phase === "expired" || phase === "error";
}
