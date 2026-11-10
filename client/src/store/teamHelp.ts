import { createSignal } from "solid-js";

export type HelpLanguage = "en" | "ru";

/** Native display language wins over the WebView fallback; regional formats are not an input. */
export function guideLanguage(systemLanguage: string, browserLanguage = "en"): HelpLanguage {
  return /^ru(?:[-_@.]|$)/i.test((systemLanguage.trim() || browserLanguage).trim()) ? "ru" : "en";
}

const browserLocale = () => typeof navigator === "undefined" ? "en" : navigator.language;
const [language, setLanguage] = createSignal<HelpLanguage>(guideLanguage("", browserLocale()));
const [opened, setOpened] = createSignal(false);
let returnFocus: HTMLElement | undefined;

export const teamHelpLanguage = language;
export const teamHelpOpen = opened;
export const setTeamHelpSystemLanguage = (systemLanguage: string) => setLanguage(guideLanguage(systemLanguage, browserLocale()));
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
