import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { useEffect, useState } from "react";
import type { PlatformInfo } from "../lib/platform";
import { api, closeWindow, isTauri } from "../lib/tauri";
import { DropzoneView, type UpdateState } from "./DropzoneView";

/** Rust reports the update download as `[bytes done, total bytes or null]`. */
type UpdateProgress = [number, number | null];

/** Wires the drop zone (app started without a file) to the Rust side. */
export function DropzoneWindow({ platform }: { platform: PlatformInfo }) {
  const [dragging, setDragging] = useState(false);
  const [update, setUpdate] = useState<UpdateState | null>(null);

  // Quietly look for a newer version; offline simply means no hint.
  useEffect(() => {
    if (!isTauri()) return;
    let active = true;
    api.checkUpdate().then(
      (found) => {
        if (active && found) setUpdate({ phase: "available", version: found.version });
      },
      () => {},
    );
    return () => {
      active = false;
    };
  }, []);

  const installUpdate = () => {
    setUpdate({ phase: "downloading", percent: 0 });
    const sub = listen<UpdateProgress>("update://progress", (e) => {
      const [done, total] = e.payload;
      setUpdate({
        phase: "downloading",
        percent: total ? Math.min(100, Math.round((done / total) * 100)) : null,
      });
    });
    // On success the app restarts; we only get here on errors.
    api.installUpdate().catch((code: unknown) => {
      // "busy": a transfer is running -- updates never cut it off.
      setUpdate({ phase: "failed", busy: code === "busy" });
      void sub.then((unlisten) => unlisten());
    });
  };

  useEffect(() => {
    if (!isTauri()) return;
    const sub = getCurrentWebview().onDragDropEvent((e) => {
      const p = e.payload;
      if (p.type === "enter" || p.type === "over") setDragging(true);
      else if (p.type === "leave") setDragging(false);
      else if (p.type === "drop") {
        setDragging(false);
        if (p.paths.length) void api.sharePaths(p.paths);
      }
    });
    return () => {
      void sub.then((unlisten) => unlisten());
    };
  }, []);

  // Ctrl+V / Cmd+V sends the clipboard text. Rust reads the clipboard, so
  // this also works where the webview fires no paste event (no text field).
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!(e.ctrlKey || e.metaKey) || e.key.toLowerCase() !== "v") return;
      e.preventDefault();
      // Holding the keys repeats the event: start one share, not several.
      if (!e.repeat) void api.shareClipboard();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  return (
    <DropzoneView
      os={platform.os}
      dragging={dragging}
      onClose={() => void closeWindow()}
      onPick={() => void api.pickFiles()}
      onClipboard={() => void api.shareClipboard()}
      onReceive={() => void api.receiveFromPhone()}
      update={update}
      onUpdate={installUpdate}
    />
  );
}
