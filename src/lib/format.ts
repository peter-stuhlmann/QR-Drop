import { lang, plural, t, type Lang } from "../i18n";
import type { FileKind, ShareInfo } from "./tauri";

const UNITS = ["B", "KB", "MB", "GB", "TB"];

/** Decimal units like Finder/iOS: 4,2 MB. */
export function formatBytes(bytes: number, l: Lang = lang): string {
  let n = Math.max(0, bytes);
  let u = 0;
  while (n >= 1000 && u < UNITS.length - 1) {
    n /= 1000;
    u++;
  }
  const digits = u === 0 || n >= 100 ? 0 : 1;
  const num = new Intl.NumberFormat(l, {
    minimumFractionDigits: digits,
    maximumFractionDigits: digits,
  }).format(n);
  return `${num} ${UNITS[u]}`;
}

/** Remaining time for the transfer: "12 Sekunden", "3 Minuten". */
export function formatEta(seconds: number, l: Lang = lang): string {
  const s = Math.max(1, Math.round(seconds));
  if (s < 60) return plural(s, "time.seconds.one", "time.seconds.many", l);
  const m = Math.round(s / 60);
  return plural(m, "time.minutes.one", "time.minutes.many", l);
}

/** Countdown "9:24". */
export function formatClock(ms: number): string {
  const total = Math.max(0, Math.ceil(ms / 1000));
  const m = Math.floor(total / 60);
  const s = total % 60;
  return `${m}:${s.toString().padStart(2, "0")}`;
}

/** Header meta of a file share: "4,2 MB · Bild" or "1,2 GB · 86 Dateien". */
export function describeFile(
  info: { size: number; kind: FileKind; isZip: boolean; fileCount: number },
  l: Lang = lang,
): string {
  const size = formatBytes(info.size, l);
  if (info.isZip) {
    return `${size} · ${plural(info.fileCount, "files.one", "files.many", l)}`;
  }
  // Checked by the compiler: every FileKind needs a `kind.*` key in the i18n files.
  return `${size} · ${t(`kind.${info.kind}`, {}, l)}`;
}

/**
 * Shortens long names in the middle so the extension stays visible:
 * "Urlaubsfotos_Mallorca_…_2026.jpg".
 */
export function middleEllipsis(name: string, max = 32): string {
  if (name.length <= max) return name;
  const dot = name.lastIndexOf(".");
  const ext = dot > 0 && name.length - dot <= 8 ? name.slice(dot) : "";
  const stem = ext ? name.slice(0, dot) : name;
  const room = max - ext.length - 1;
  const head = Math.ceil(room * 0.65);
  const tail = room - head;
  return `${stem.slice(0, head)}…${stem.slice(stem.length - tail)}${ext}`;
}

/** Host of a link for the window title, falls back to the raw text. */
export function linkHost(url: string): string {
  try {
    return new URL(url).host || url;
  } catch {
    return url;
  }
}

/** Title + subtitle of the share window header for files, texts and links. */
export function shareHeader(
  info: Pick<
    ShareInfo,
    | "name"
    | "size"
    | "kind"
    | "isZip"
    | "fileCount"
    | "content"
    | "secret"
    | "charCount"
    | "preview"
    | "ttlMs"
  >,
  l: Lang = lang,
): { title: string; meta: string } {
  if (info.content === "url") {
    const url = info.preview ?? info.name;
    return { title: linkHost(url), meta: middleEllipsis(url, 44) };
  }
  if (info.content === "text") {
    const chars = plural(info.charCount, "content.chars.one", "content.chars.many", l);
    if (info.secret) {
      return {
        title: t("kind.secret", {}, l),
        meta: t("content.secretMeta", { min: Math.round(info.ttlMs / 60_000) }, l),
      };
    }
    return {
      title: info.preview || t("kind.text", {}, l),
      meta: t("content.textMeta", { chars }, l),
    };
  }
  return { title: middleEllipsis(info.name, 34), meta: describeFile(info, l) };
}
