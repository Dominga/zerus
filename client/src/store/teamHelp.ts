import { createSignal } from "solid-js";

export type HelpLanguage = "en" | "ru";

/** Native OS locale wins over WebView2's language and the saved UI language. */
export function guideLanguage(locale: string, browserLocale = "en"): HelpLanguage {
  return /^ru(?:[-_@.]|$)/i.test((locale.trim() || browserLocale).trim()) ? "ru" : "en";
}

const browserLocale = () => typeof navigator === "undefined" ? "en" : navigator.language;
const [language, setLanguage] = createSignal<HelpLanguage>(guideLanguage("", browserLocale()));
const [opened, setOpened] = createSignal(false);
let returnFocus: HTMLElement | undefined;

export const teamHelpLanguage = language;
export const teamHelpOpen = opened;
export const setTeamHelpLocale = (locale: string) => setLanguage(guideLanguage(locale, browserLocale()));
export function openTeamHelp() {
  if (opened()) return;
  returnFocus = typeof document !== "undefined" && document.activeElement instanceof HTMLElement ? document.activeElement : undefined;
  setOpened(true);
}
export function closeTeamHelp() {
  if (!opened()) return;
  setOpened(false);
  const target = returnFocus;
  returnFocus = undefined;
  queueMicrotask(() => { if (target?.isConnected) target.focus(); });
}

export const teamHelpLabel = () => language() === "ru" ? "Справка по командам" : "Team guide";
