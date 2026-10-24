/** Typed bridge to the Rust side (src-tauri/src/commands.rs + events). */
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";

export type FileKind =
  | "image"
  | "video"
  | "audio"
  | "document"
  | "archive"
  | "file"
  | "text"
  | "secret"
  | "link"
  | "receive";
/** `receive` = the other direction: the phone sends to this computer. */
export type ContentType = "file" | "text" | "url" | "receive";
export type ShareStatus = "preparing" | "ready" | "expired" | "error";
/**
 * Error codes the Rust side sends, either as `ShareInfo.error` or as a
 * rejected command. Keep in sync with the ERRORS map in errors.ts.
 */
export type KnownErrorReason =
  | "no_network"
  | "not_found"
  | "qr"
  | "unknown_share"
  | "clipboard_empty"
  | "text_too_large"
  | "generic";
/** `string & {}` keeps editor completion for the known codes but accepts new ones. */
export type ErrorReason = KnownErrorReason | (string & {});
export type IfaceKind = "wifi" | "ethernet" | "other";

export interface NetworkOption {
  ip: string;
  name: string;
  kind: IfaceKind;
}

export interface ShareInfo {
  id: string;
  status: ShareStatus;
  error: ErrorReason | null;
  name: string;
  size: number;
  kind: FileKind;
  isZip: boolean;
  fileCount: number;
  content: ContentType;
  /** Password/token: masked everywhere, valid for 2 minutes only. */
  secret: boolean;
  charCount: number;
  /** First line of a text (never for secrets) or the link. */
  preview: string | null;
  url: string | null;
  qrSvg: string | null;
  expiresInMs: number;
  ttlMs: number;
  networks: NetworkOption[];
  selectedIp: string | null;
}

/** A file of a batch the phone is uploading right now. */
export interface UploadPayload {
  id: string;
  name: string;
  index: number;
  count: number;
  sent: number;
  total: number;
  bytesPerSec: number;
}

/** A file from the phone was saved. */
export interface ReceivedPayload {
  id: string;
  name: string;
  size: number;
  kind: FileKind;
}

/** A text from the phone went into the clipboard. */
export interface TextReceivedPayload {
  id: string;
  chars: number;
  /** First line; null for passwords. */
  preview: string | null;
}

/** A newer QR Drop version (from GitHub releases). */
export interface UpdateInfo {
  version: string;
}

export interface ProgressPayload {
  id: string;
  sent: number;
  total: number;
  bytesPerSec: number;
}

/** True inside the Tauri webview (false in the browser demo). */
export const isTauri = (): boolean =>
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export const api = {
  getShare: (id: string) => invoke<ShareInfo>("get_share", { id }),
  selectNetwork: (id: string, ip: string) => invoke<ShareInfo>("select_network", { id, ip }),
  renewShare: (id: string) => invoke<ShareInfo>("renew_share", { id }),
  cancelShare: (id: string) => invoke<void>("cancel_share", { id }),
  sharePaths: (paths: string[]) => invoke<void>("share_paths", { paths }),
  shareClipboard: () => invoke<void>("share_clipboard"),
  /** Native file dialog; folders arrive by drag & drop, so the folder mode stays off. */
  pickFiles: () => invoke<boolean>("pick_files", { folder: false }),
  finishWelcome: () => invoke<void>("finish_welcome"),
  receiveFromPhone: () => invoke<void>("receive_from_phone"),
  revealReceived: (id: string) => invoke<void>("reveal_received", { id }),
  checkUpdate: () => invoke<UpdateInfo | null>("check_update"),
  installUpdate: () => invoke<void>("install_update"),
};

export type ShareEventName =
  | "share://connected"
  | "share://progress"
  | "share://interrupted"
  | "share://done"
  | "share://expired"
  | "share://update"
  | "share://upload"
  | "share://received"
  | "share://text";

/** Listens to an event addressed to this window only. */
export function onShareEvent<T = unknown>(
  name: ShareEventName,
  handler: (payload: T) => void,
): Promise<UnlistenFn> {
  const label = getCurrentWebviewWindow().label;
  return listen<T>(name, (e) => handler(e.payload), { target: { kind: "WebviewWindow", label } });
}

export function closeWindow(): Promise<void> {
  return getCurrentWebviewWindow().close();
}
