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
 * paste event reaches Terminal's paste handler, which stages an event image
 * for this attach. Ctrl+C and Ctrl+V stay the agent's keys.
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

/** The event source and its normalized PNG each stay within the native attachment bound. */
export const IMAGE_BYTES = 10 * 1024 * 1024;
const IMAGE_PIXELS = 32_000_000;
const unreadable = () => new Error(tr("Could not read the pasted image."));

function pngDimensions(bytes: Uint8Array) {
  if (bytes.length < 24 || ![137, 80, 78, 71, 13, 10, 26, 10].every((b, i) => bytes[i] === b)
    || String.fromCharCode(...bytes.subarray(12, 16)) !== "IHDR") throw unreadable();
  const data = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  dimensions(data.getUint32(16), data.getUint32(20));
}
function dimensions(width: number, height: number) {
  if (width <= 0 || height <= 0 || width * height > IMAGE_PIXELS) throw new Error(tr("Image is too large. Use an image below 32 megapixels."));
}

/** No clipboard read or composer state: only the File captured from this paste event. */
export async function normalizeTerminalImage(file: Blob): Promise<Uint8Array> {
  if (!file.size || file.size > IMAGE_BYTES) throw new Error(tr("Each file must be readable, nonempty, and at most 10 MiB."));
  if (file.type === "image/png") {
    const bytes = new Uint8Array(await file.arrayBuffer());
    if (!bytes.length || bytes.length > IMAGE_BYTES) throw unreadable();
    pngDimensions(bytes);
    return bytes;
  }
  const bitmap = await createImageBitmap(file);
  try {
    dimensions(bitmap.width, bitmap.height);
    const canvas = new OffscreenCanvas(bitmap.width, bitmap.height);
    const context = canvas.getContext("2d");
    if (!context) throw unreadable();
    context.drawImage(bitmap, 0, 0);
    const png = await canvas.convertToBlob({ type: "image/png" });
    if (!png.size || png.size > IMAGE_BYTES) throw unreadable();
    const bytes = new Uint8Array(await png.arrayBuffer());
    pngDimensions(bytes);
    return bytes;
  } finally { bitmap.close(); }
}

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
