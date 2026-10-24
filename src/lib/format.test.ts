import { describe, expect, it } from "vitest";
import { detectLang, t } from "../i18n";
import {
  describeFile,
  formatBytes,
  formatClock,
  formatEta,
  linkHost,
  middleEllipsis,
  shareHeader,
} from "./format";
import { isWindows11 } from "./platform";

describe("format", () => {
  it("formats bytes with locale decimals", () => {
    expect(formatBytes(0, "de")).toBe("0 B");
    expect(formatBytes(4_200_000, "de")).toBe("4,2 MB");
    expect(formatBytes(4_200_000, "en")).toBe("4.2 MB");
    expect(formatBytes(1_234_000_000, "de")).toBe("1,2 GB");
    expect(formatBytes(512_000, "de")).toBe("512 KB");
  });

  it("formats eta and clock", () => {
    expect(formatEta(0.4, "de")).toBe("1 Sekunde");
    expect(formatEta(12, "de")).toBe("12 Sekunden");
    expect(formatEta(130, "en")).toBe("2 minutes");
    expect(formatClock(564_000)).toBe("9:24");
    expect(formatClock(0)).toBe("0:00");
  });

  it("describes files and zips", () => {
    expect(describeFile({ size: 4_200_000, kind: "image", isZip: false, fileCount: 1 }, "de")).toBe(
      "4,2 MB · Bild",
    );
    expect(
      describeFile({ size: 1_200_000_000, kind: "archive", isZip: true, fileCount: 86 }, "de"),
    ).toBe("1,2 GB · 86 Dateien");
  });

  it("shortens long names in the middle and keeps the extension", () => {
    const name = "Urlaubsfotos_Mallorca_Strand_Sonnenuntergang_2026.jpg";
    const short = middleEllipsis(name, 32);
    expect(short.length).toBe(32);
    expect(short.endsWith("2026.jpg")).toBe(true);
    expect(short).toContain("…");
    expect(middleEllipsis("kurz.pdf")).toBe("kurz.pdf");
  });

  it("detects language and fills placeholders", () => {
    expect(detectLang("de-AT")).toBe("de");
    expect(detectLang("en-US")).toBe("en");
    expect(detectLang("fr-FR")).toBe("en");
    expect(t("share.done.autoclose", { s: 3 }, "de")).toBe("Schließt automatisch in 3 s");
  });

  it("builds headers for texts, secrets and links", () => {
    const base = {
      name: "Text",
      size: 10,
      kind: "text" as const,
      isZip: false,
      fileCount: 0,
      secret: false,
      charCount: 128,
      preview: "Einkaufsliste",
      ttlMs: 600_000,
    };
    expect(shareHeader({ ...base, content: "text" }, "de")).toEqual({
      title: "Einkaufsliste",
      meta: "Text · 128 Zeichen",
    });
    const secret = shareHeader(
      { ...base, content: "text", secret: true, preview: null, ttlMs: 120_000 },
      "de",
    );
    expect(secret.title).toBe("Passwort");
    expect(secret.meta).toContain("2 Min.");
    expect(JSON.stringify(secret)).not.toContain("Einkauf");
    const link = shareHeader(
      { ...base, content: "url", preview: "https://www.example.com/rezepte?x=1" },
      "de",
    );
    expect(link.title).toBe("www.example.com");
    expect(linkHost("kein link")).toBe("kein link");
  });

  it("recognises Windows 11 builds", () => {
    expect(isWindows11("10.0.22631")).toBe(true);
    expect(isWindows11("10.0.19045")).toBe(false);
  });
});
