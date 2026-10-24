import { useCallback, useReducer, useRef } from "react";
import type { PlatformInfo } from "../lib/platform";
import {
  api,
  onShareEvent,
  type ReceivedPayload,
  type TextReceivedPayload,
  type UploadPayload,
} from "../lib/tauri";
import { initialReceiveState, receiveReducer } from "../state/receiveMachine";
import { useShareSession } from "../state/useShareSession";
import { ReceiveView } from "./ReceiveView";

/** Wires the receive window (phone → computer) to the Rust side. */
export function ReceiveWindow({ id, platform }: { id: string; platform: PlatformInfo }) {
  const [state, dispatch] = useReducer(receiveReducer, initialReceiveState);
  // Counts arrivals for the list keys. A ref, not a variable inside
  // `subscribe`: a re-subscription must not start again at 1 and reuse keys.
  const arrivals = useRef(0);

  const subscribe = useCallback(() => {
    const nextKey = (prefix: string) => `${prefix}${++arrivals.current}`;
    return [
      onShareEvent<UploadPayload>("share://upload", (u) =>
        dispatch({
          type: "upload",
          name: u.name,
          index: u.index,
          count: u.count,
          sent: u.sent,
          total: u.total,
          bytesPerSec: u.bytesPerSec,
        }),
      ),
      onShareEvent<ReceivedPayload>("share://received", (r) =>
        dispatch({
          type: "received",
          item: { key: nextKey("f"), kind: r.kind, name: r.name, size: r.size },
        }),
      ),
      onShareEvent<TextReceivedPayload>("share://text", (r) =>
        dispatch({
          type: "received",
          item: {
            key: nextKey("t"),
            kind: r.preview === null ? "secret" : "text",
            name: r.preview ?? "",
            size: r.chars,
          },
        }),
      ),
    ];
  }, []);

  const session = useShareSession({ id, phase: state.view.phase, dispatch, subscribe });

  return (
    <ReceiveView
      os={platform.os}
      state={state}
      onOpenFolder={() => void api.revealReceived(id)}
      {...session}
    />
  );
}
