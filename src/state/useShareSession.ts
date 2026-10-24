/**
 * Wiring shared by the share and the receive window: loads the session from
 * Rust, follows its events, runs the countdown and the firewall timer and
 * offers the user actions (cancel, close, renew, retry, other network).
 */
import type { UnlistenFn } from "@tauri-apps/api/event";
import { useCallback, useEffect, useState } from "react";
import { useEscape, useNow } from "../lib/hooks";
import { api, closeWindow, onShareEvent, type ShareInfo } from "../lib/tauri";
import { isSettled, type PhaseName, type SessionAction } from "./session";

/** No request this long in "ready": show the firewall / Wi-Fi hint (CLAUDE.md §5.4). */
const FIREWALL_HINT_AFTER_MS = 60_000;
/** The countdown shows whole seconds. */
const COUNTDOWN_TICK_MS = 1000;

/** Everything a share/receive view needs from its session, named to spread as props. */
export interface SessionViewProps {
  info: ShareInfo | null;
  /** Time left until the QR code expires. */
  remainingMs: number;
  showFirewallHint: boolean;
  onClose: () => void;
  onCancel: () => void;
  /** Title-bar button and Esc: close when settled, cancel otherwise. */
  onDismiss: () => void;
  onRenew: () => void;
  onRetry: () => void;
  onSelectNetwork: (ip: string) => void;
}

interface Options {
  /** Share id from the window URL. */
  id: string;
  /** Current phase of the window's state machine. */
  phase: PhaseName;
  /** The window's reducer dispatch. */
  dispatch: (action: SessionAction) => void;
  /**
   * Registers the window-specific events. Must be stable (useCallback),
   * otherwise every render subscribes again.
   */
  subscribe: () => Promise<UnlistenFn>[];
}

/** The latest ShareInfo and when it arrived (its `expiresInMs` counts from then). */
interface Snapshot {
  info: ShareInfo;
  receivedAt: number;
}

export function useShareSession({ id, phase, dispatch, subscribe }: Options): SessionViewProps {
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null);
  /** QR URL of the "ready" phase for which the firewall timer fired. */
  const [firewallFor, setFirewallFor] = useState<string | null>(null);
  const ready = phase === "ready";
  // Only the "ready" footer shows the countdown, so only then the clock ticks.
  const now = useNow(ready ? COUNTDOWN_TICK_MS : null);

  const apply = useCallback(
    (info: ShareInfo) => {
      setSnapshot({ info, receivedAt: Date.now() });
      dispatch({ type: "info", info });
    },
    [dispatch],
  );

  const fail = useCallback(
    (e: unknown) => dispatch({ type: "failed", reason: typeof e === "string" ? e : "generic" }),
    [dispatch],
  );

  const refresh = useCallback(() => {
    api.getShare(id).then(apply, fail);
  }, [id, apply, fail]);

  useEffect(() => {
    let active = true;
    const subs = [
      onShareEvent("share://update", refresh),
      onShareEvent("share://connected", () => dispatch({ type: "connected" })),
      onShareEvent("share://interrupted", () => dispatch({ type: "interrupted" })),
      onShareEvent("share://expired", () => dispatch({ type: "expired" })),
      ...subscribe(),
    ];
    // Listen first, then load: an update sent between loading and listening
    // would otherwise be lost and the window would show stale data.
    void Promise.all(subs).then(() => {
      if (active) refresh();
    });
    return () => {
      active = false;
      subs.forEach((sub) => void sub.then((unlisten) => unlisten()));
    };
  }, [refresh, subscribe, dispatch]);

  const readyKey = ready ? (snapshot?.info.url ?? "") : null;
  useEffect(() => {
    if (readyKey === null) return;
    const timer = setTimeout(() => setFirewallFor(readyKey), FIREWALL_HINT_AFTER_MS);
    return () => clearTimeout(timer);
  }, [readyKey]);

  const cancel = useCallback(() => {
    dispatch({ type: "cancel" });
    void api.cancelShare(id).finally(() => closeWindow());
  }, [id, dispatch]);

  const close = useCallback(() => void closeWindow(), []);

  const dismiss = isSettled(phase) ? close : cancel;
  useEscape(dismiss);

  // `now` may predate the snapshot while the clock is paused: count from arrival.
  const elapsedMs = snapshot ? Math.max(0, now - snapshot.receivedAt) : 0;

  return {
    info: snapshot?.info ?? null,
    remainingMs: snapshot ? Math.max(0, snapshot.info.expiresInMs - elapsedMs) : 0,
    showFirewallHint: readyKey !== null && firewallFor === readyKey,
    onClose: close,
    onCancel: cancel,
    onDismiss: dismiss,
    onRenew: () => void api.renewShare(id).then(apply, fail),
    onRetry: refresh,
    onSelectNetwork: (ip) => void api.selectNetwork(id, ip).then(apply, fail),
  };
}
