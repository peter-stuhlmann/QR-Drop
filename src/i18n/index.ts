import de from "./de.json";
import en from "./en.json";

export type Lang = "de" | "en";
export type MessageKey = keyof typeof de;

const messages: Record<Lang, Record<MessageKey, string>> = { de, en };

export function detectLang(locale: string | undefined = globalThis.navigator?.language): Lang {
  return (locale ?? "de").toLowerCase().startsWith("de") ? "de" : "en";
}

export const lang: Lang = detectLang();

/** Translates a key and fills `{placeholders}`. */
export function t(
  key: MessageKey,
  vars: Record<string, string | number> = {},
  l: Lang = lang,
): string {
  const text = messages[l][key] ?? messages.de[key] ?? key;
  return text.replace(/\{(\w+)\}/g, (_, name: string) =>
    name in vars ? String(vars[name]) : `{${name}}`,
  );
}

/** "1 Datei" / "3 Dateien" style helper. */
export function plural(n: number, one: MessageKey, many: MessageKey, l: Lang = lang): string {
  return n === 1 ? t(one, {}, l) : t(many, { n }, l);
}
