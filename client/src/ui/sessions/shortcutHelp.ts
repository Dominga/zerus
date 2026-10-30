import { COMMANDS, keys, nativeText, parseSequence, platform, type Platform } from "../../store/shortcuts";

export interface ShortcutHelpRow { group: string; title: string; chord: string }

const native = (text: string) => nativeText(parseSequence(text)!);

export function fixedShortcutRows(on: Platform = platform): ShortcutHelpRow[] {
  const terminalCopy = on === "macos" ? "⌘C, ⌘V" : "Ctrl+Shift+C, Ctrl+Shift+V";
  return [
    { group: "Fixed keys", title: "Send the message", chord: native("Return") },
    { group: "Fixed keys", title: "New line in the message field", chord: native("Shift+Return") },
    { group: "Fixed keys", title: "Clear the search, then stop the selected turn; close dialogs", chord: native("Escape") },
    { group: "Fixed keys", title: "Move focus", chord: `${native("Tab")}, ${native("Shift+Tab")}` },
    { group: "Fixed keys", title: "Take the suggestion in an empty message field", chord: native("Tab") },
    { group: "Fixed keys", title: "Move and open in lists", chord: `${native("Up")} ${native("Down")} ${native("Home")} ${native("End")} ${native("Return")}` },
    { group: "Fixed keys", title: "Context menu", chord: `${native("Menu")}, ${native("Shift+F10")}` },
    { group: "Fixed keys", title: "Select several items", chord: `${on === "macos" ? "⌘" : "Ctrl"}-click` },
    { group: "Fixed keys", title: "Stop voice capture", chord: `${native("Space")}, ${native("Escape")}` },
    { group: "Fixed keys", title: "Copy and paste in Terminal", chord: terminalCopy },
    { group: "Fixed keys", title: "Scroll Terminal", chord: `${native("Shift+PageUp")}, ${native("Shift+PageDown")}` },
    { group: "Fixed keys", title: "Reorder projects in the Projects dialog", chord: `${native("Alt+Up")}, ${native("Alt+Down")}` },
  ];
}

export function shortcutHelpRows(on: Platform = platform): ShortcutHelpRow[] {
  return [
    ...COMMANDS.map((command) => ({ group: command.group, title: command.title, chord: keys(command.id, on).map((key) => nativeText(key, on)).join(", ") })),
    ...fixedShortcutRows(on),
  ];
}

export function filterShortcutRows(rows: readonly ShortcutHelpRow[], needle: string): ShortcutHelpRow[] {
  const query = needle.trim().toLowerCase();
  if (!query) return [...rows];
  return rows.filter((row) => row.title.toLowerCase().includes(query) || row.chord.toLowerCase().includes(query));
}
