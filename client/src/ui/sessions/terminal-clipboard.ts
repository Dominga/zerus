import { tr } from "../../i18n";
import type { MenuEntry } from "../../store/ui";
import type { Platform } from "../../store/shortcuts";

// Terminal's own copy and paste (Qt's TerminalScreen copyKey, pasteKey and
// contextMenuEvent, TerminalScreen.cpp:28-46,699-708).

type Keys = Pick<KeyboardEvent, "key" | "code" | "ctrlKey" | "metaKey" | "altKey" | "shiftKey">;

/** The layout's own letter, or the key's position for another script (Cyrillic), as the shortcut router reads it. */
const letter = (event: Keys, wanted: string) =>
  /^[\x21-\x7e]$/.test(event.key) ? event.key.toUpperCase() === wanted : event.code === `Key${wanted}`;

/**
 * Whether a key copies Terminal's selection (Qt's copyKey): Command-C on
 * macOS, Ctrl+Shift+C elsewhere. xterm sends nothing for Ctrl+Shift+C, so
 * without this the chord did nothing.
 */
export function copyKey(event: Keys, on: Platform): boolean {
  if (event.altKey || !letter(event, "C")) return false;
  return on === "macos" ? event.metaKey && !event.ctrlKey && !event.shiftKey : event.ctrlKey && event.shiftKey && !event.metaKey;
}

/**
 * xterm's custom key handler: the copy key never reaches the agent. Paste
 * keys stay with the engine (Qt's pasteKey: Ctrl+Shift+V and Shift+Insert,
 * Command-V on macOS): xterm sends nothing for them and the engine's own
 * paste event reaches Terminal's paste handler, which also takes an image
 * to the message field. Ctrl+C and Ctrl+V stay the agent's keys.
 */
export function clipboardKeyHandler(on: Platform, copy: () => void) {
  return (event: KeyboardEvent): boolean => {
    if (!copyKey(event, on)) return true;
    if (event.type === "keydown") { event.preventDefault(); copy(); }
    return false;
  };
}

/** The chords the menu shows, as Qt's shortcut help writes them. */
export const clipboardChords = (on: Platform) => on === "macos"
  ? { copy: "⌘C", paste: "⌘V" }
  : { copy: "Ctrl+Shift+C", paste: "Ctrl+Shift+V" };

/**
 * The right-click menu, or undefined while the agent reads the mouse and
 * Shift is not held: then the click is the agent's, as in Qt.
 */
export function terminalMenu(state: { selection: boolean; mouseApp: boolean; shift: boolean; on: Platform },
  copy: () => void, paste: () => void): MenuEntry[] | undefined {
  if (state.mouseApp && !state.shift) return undefined;
  const chords = clipboardChords(state.on);
  return [
    { label: tr("Copy selection"), icon: "copy", kbd: chords.copy, disabled: !state.selection, action: state.selection ? copy : undefined },
    { label: tr("Paste"), kbd: chords.paste, action: paste },
  ];
}
