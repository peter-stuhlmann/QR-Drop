/** Maps backend error codes to UI texts and decides whether retrying helps. */
import type { ErrorReason, KnownErrorReason } from "./tauri";

/** Message group in the i18n files: `error.<key>.title|text|hint`. */
export type ErrorMessageKey =
  "no_network" | "not_found" | "clipboard_empty" | "text_too_large" | "generic";

export interface ErrorDetails {
  key: ErrorMessageKey;
  /** Only offer "Erneut versuchen" when a second attempt can succeed. */
  retryable: boolean;
}

/** One entry per known code: the Record type makes a missing code a compile error. */
const ERRORS: Record<KnownErrorReason, ErrorDetails> = {
  // The user can fix this (join the Wi-Fi) and try again.
  no_network: { key: "no_network", retryable: true },
  not_found: { key: "not_found", retryable: false },
  clipboard_empty: { key: "clipboard_empty", retryable: false },
  text_too_large: { key: "text_too_large", retryable: false },
  // The data does not fit into a QR code; the same data fails again.
  qr: { key: "generic", retryable: false },
  // The share is gone in the backend; there is nothing left to retry.
  unknown_share: { key: "generic", retryable: false },
  generic: { key: "generic", retryable: true },
};

/** `hasOwn` instead of `in`: "toString" must not count as a known code. */
const isKnown = (reason: ErrorReason): reason is KnownErrorReason => Object.hasOwn(ERRORS, reason);

/** Texts and retry policy for an error code; unknown codes count as generic. */
export function describeError(reason: ErrorReason): ErrorDetails {
  return isKnown(reason) ? ERRORS[reason] : ERRORS.generic;
}
