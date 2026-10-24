import { useCallback, useEffect, useReducer, useState } from "react";
import type { PlatformInfo } from "../lib/platform";
import { closeWindow, onShareEvent, type ProgressPayload } from "../lib/tauri";
import { initialState, shareReducer } from "../state/shareMachine";
import { useShareSession } from "../state/useShareSession";
import { ShareView } from "./ShareView";

/** After a finished transfer the window closes itself (CLAUDE.md §3.2). */
const AUTO_CLOSE_S = 3;

/** Seconds until the window closes itself; counts down once `active`. */
function useAutoClose(active: boolean): number {
  const [secondsLeft, setSecondsLeft] = useState(AUTO_CLOSE_S);
  useEffect(() => {
    if (!active) return;
    let left = AUTO_CLOSE_S;
    const timer = setInterval(() => {
      left -= 1;
      setSecondsLeft(left);
      if (left <= 0) {
        clearInterval(timer);
        void closeWindow();
      }
    }, 1000);
    return () => clearInterval(timer);
  }, [active]);
  return secondsLeft;
}

/** Wires the share window (computer → phone) to the Rust side. */
export function ShareWindow({ id, platform }: { id: string; platform: PlatformInfo }) {
  const [state, dispatch] = useReducer(shareReducer, initialState);

  const subscribe = useCallback(
    () => [
      onShareEvent<ProgressPayload>("share://progress", (p) =>
        dispatch({ type: "progress", sent: p.sent, total: p.total, bytesPerSec: p.bytesPerSec }),
      ),
      onShareEvent("share://done", () => dispatch({ type: "done" })),
    ],
    [],
  );

  const session = useShareSession({ id, phase: state.phase, dispatch, subscribe });
  const autoCloseIn = useAutoClose(state.phase === "done");

  return <ShareView os={platform.os} state={state} autoCloseIn={autoCloseIn} {...session} />;
}
