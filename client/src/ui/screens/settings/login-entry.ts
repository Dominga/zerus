// Settings: Start Zerus at login. The native side reads and writes the app's own
// Run value and is the only source of its state; the page sends only on or off.
import { createSignal } from "solid-js";
import { tr } from "../../../i18n";
import type { AutostartLine } from "./connector-autostart";

export type LoginState = "on" | "off" | "other" | "unknown";
export interface LoginEntry { available: boolean; state: LoginState }
export interface LoginEntryIo { read(): Promise<LoginEntry>; write(enabled: boolean): Promise<LoginEntry> }

export function createLoginEntry(io: LoginEntryIo) {
  const [entry, setEntry] = createSignal<LoginEntry>();
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal("");
  // A read that began before a newer answer must not replace it.
  let newest = 0;
  const read = async () => {
    const version = ++newest;
    try {
      const answer = await io.read();
      if (version === newest) setEntry(answer);
    } catch {
      if (version === newest) setEntry(undefined);
    }
  };
  return {
    state: () => entry()?.state,
    available: () => !!entry()?.available,
    checked: () => entry()?.state === "on",
    disabled: () => busy() || !entry()?.available || entry()?.state === "unknown",
    error,
    async load() {
      setError("");
      await read();
    },
    async choose(enabled: boolean) {
      if (busy()) return;
      setBusy(true);
      setError("");
      const version = ++newest;
      try {
        const answer = await io.write(enabled);
        if (version === newest) setEntry(answer);
      } catch (failure) {
        setError(String(failure));
        await read();
      } finally {
        setBusy(false);
      }
    },
  };
}

export function loginEntryHint(state?: LoginState): string {
  const base = tr("Starts Zerus in the notification area when you sign in to Windows. This setting doesn't detect or remove a shortcut in the Windows Startup folder.");
  if (state === "other") return `${base} ${tr("The login entry points to another copy of Zerus or has a different command, so it isn't confirmed for this copy. Turn this on to replace it.")}`;
  if (state === "unknown") return `${base} ${tr("Zerus can't read its login entry right now.")}`;
  return base;
}

// Only a connector the native side will start may be promised. Otherwise the line
// describes Zerus alone, so it can't contradict "Stopped by you" just above it.
const STARTS: ReadonlySet<AutostartLine["kind"]> = new Set<AutostartLine["kind"]>(["running", "starting", "will_start", "failed"]);

export function loginEntryMobileLine(state: LoginState, line?: AutostartLine | null): string {
  const starts = !!line && STARTS.has(line.kind);
  switch (state) {
    case "on": return starts ? tr("Zerus starts at login on this computer, and the connector starts with it.") : tr("Zerus starts at login on this computer.");
    case "off": return starts ? tr("Start Zerus at login is off, so the connector starts when you open Zerus, unless a Startup shortcut opens it.") : tr("Start Zerus at login is off, so Zerus starts when you open it, unless a Startup shortcut opens it.");
    default: return starts ? tr("Zerus can't confirm that it starts at login on this computer, so the connector may wait until you open Zerus.") : tr("Zerus can't confirm that it starts at login on this computer.");
  }
}
