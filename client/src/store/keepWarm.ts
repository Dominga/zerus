import { invoke } from "@tauri-apps/api/core";
import { settings, setSetting } from "./settings";
import { desktopService } from "./desktopService";
import { KEEP_WARM_KEY, MAX_HOURS, hoursOf, parseHours, warmable } from "../ui/sessions/keep-warm";
import type { Session } from "../ui/sessions/types";

// Keep cache warm's choice per session (Qt's CacheKeepalive::hours/setHours).
// Read from the settings store, which follows every change Rust makes; written
// through `keep_warm_set`, which checks the listed row and keeps Qt's bound.
// Rust's own worker sends the keepalives, also while every window is closed.

// Parsed once per stored value; reading the setting keeps callers reactive.
let parsed: { raw: unknown; hours: ReadonlyMap<string, number> } | undefined;
const saved = () => {
  const raw = settings[KEEP_WARM_KEY];
  if (!parsed || parsed.raw !== raw) parsed = { raw, hours: parseHours(raw) };
  return parsed.hours;
};
// The bridge's own check, without loading the bridge into every session card.
const inTauri = () => typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

/** The session's hours, 0 when its cache is not kept warm. */
export const keepWarmHours = (s: Session): number => hoursOf(saved(), s.compactKeys);

/** The background service has no keepalive: there the choice would send nothing. */
export const keepWarmAvailable = () => !desktopService.enabled();

/** Stores the session's hours (0 turns it off); rejects when it was not saved. */
export async function setKeepWarm(s: Session, hours: number): Promise<void> {
  if (!warmable(s)) return;
  const next = Math.min(MAX_HOURS, Math.max(0, Math.round(hours)));
  if (inTauri()) {
    await invoke<number>("keep_warm_set", { computer: s.computer, key: s.catalogKey, hours: next });
    return;
  }
  // The browser dev mode only keeps the choice; nothing is sent there.
  const values: Record<string, number> = Object.fromEntries(parseHours(settings[KEEP_WARM_KEY]));
  if (next > 0) values[s.compactKeys![0]] = next;
  else delete values[s.compactKeys![0]];
  if (!await setSetting(KEEP_WARM_KEY, JSON.stringify(values))) throw new Error("not saved");
}
