import { platform, version } from "@tauri-apps/plugin-os";
import { isTauri } from "./tauri";

export type Os = "macos" | "windows" | "other";

export interface PlatformInfo {
  os: Os;
  /** Native Mica/Vibrancy available (not on Windows 10 or in the browser). */
  nativeGlass: boolean;
}

/** Windows 11 starts with build 22000. */
export function isWindows11(ver: string): boolean {
  const build = Number(ver.split(".")[2] ?? 0);
  return build >= 22000;
}

export function detectPlatform(): PlatformInfo {
  if (!isTauri()) {
    const mac = /Mac/.test(navigator.userAgent);
    return { os: mac ? "macos" : "windows", nativeGlass: false };
  }
  const p = platform();
  if (p === "macos") return { os: "macos", nativeGlass: true };
  if (p === "windows") return { os: "windows", nativeGlass: isWindows11(version()) };
  return { os: "other", nativeGlass: false };
}

/** Adds `os-*` / `no-native-glass` classes used by the CSS. */
export function applyPlatformClasses(info: PlatformInfo, root = document.documentElement): void {
  root.classList.add(`os-${info.os}`);
  if (!info.nativeGlass) root.classList.add("no-native-glass");
}
