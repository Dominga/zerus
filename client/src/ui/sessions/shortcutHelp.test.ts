import { afterEach, describe, expect, it } from "vitest";
import { nativeText, parseSequence, reset, setKeys } from "../../store/shortcuts";
import { filterShortcutRows, fixedShortcutRows, shortcutHelpRows } from "./shortcutHelp";

describe("shortcut help", () => {
  afterEach(() => reset("go.anywhere"));

  it("includes every command and the fixed rows in Qt's order", () => {
    const rows = shortcutHelpRows();
    expect(rows[0]).toMatchObject({ group: "Go to", title: "Go to Overview", chord: "Ctrl+1" });
    expect(rows.at(-1)).toEqual({ group: "Fixed keys", title: "Reorder projects in the Projects dialog", chord: "Alt+Up, Alt+Down" });
    expect(rows.filter((row) => row.group === "Fixed keys")).toHaveLength(12);
  });

  it("filters titles and current chords without changing group order", () => {
    const rows = filterShortcutRows(shortcutHelpRows(), "ctrl+k");
    expect(rows.some((row) => row.title === "Go to…")).toBe(true);
    expect(rows.every((row) => row.title.toLowerCase().includes("ctrl+k") || row.chord.toLowerCase().includes("ctrl+k"))).toBe(true);
    expect(filterShortcutRows(shortcutHelpRows(), "voice")).toHaveLength(1);
  });

  it("shows a rebound chord through the existing registry", () => {
    const sequence = parseSequence("Ctrl+Alt+J")!;
    setKeys("go.anywhere", [sequence]);
    expect(shortcutHelpRows().find((row) => row.title === "Go to…")?.chord).toBe(nativeText(sequence));
  });

  it("uses platform-specific terminal copy and paste", () => {
    expect(fixedShortcutRows("macos").find((row) => row.title === "Copy and paste in Terminal")?.chord).toBe("⌘C, ⌘V");
    expect(fixedShortcutRows("linux").find((row) => row.title === "Copy and paste in Terminal")?.chord).toBe("Ctrl+Shift+C, Ctrl+Shift+V");
  });
});
