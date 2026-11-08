// Task #323 (N5): Qt prints usage numbers with the owner's regional format
// (QLocale::system), whose group separator may be a space. WebView2's own
// locale is its UI language, so the app passes the Windows regional format
// from the core once at start; before it arrives the browser default applies.

let locale: string | undefined;

/** The core's regional locale (for example ru-RU); empty keeps the default. */
export function setSystemLocale(value: string): void {
  const trimmed = value.trim();
  locale = trimmed || undefined;
}

/** A number in the owner's locale, as Qt's QLocale::toString would write it. */
export function numberText(value: number, options?: Intl.NumberFormatOptions): string {
  return value.toLocaleString(locale ?? "en-US", options);
}