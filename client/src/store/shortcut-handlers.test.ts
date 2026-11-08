import { describe, expect, it } from "vitest";
import { COMMANDS } from "./shortcuts";

// Task 242: every command Settings > Keyboard documents has a handler in the
// window (Qt's SessionsWindow::runShortcut runs each one), so a chord never
// does nothing. The sources register them with onCommand("<id>", ...) or as an
// App.tsx command pair ["<id>", ...].
const SOURCES = import.meta.glob(["../**/*.ts", "../**/*.tsx", "!../**/*.test.ts", "!../**/*.test.tsx", "!../perf/**"], { query: "?raw", import: "default", eager: true }) as Record<string, string>;
const registered = new Set<string>();
for (const text of Object.values(SOURCES)) {
  for (const match of text.matchAll(/onCommand\(\s*"([\w.]+)"/g)) registered.add(match[1]);
  for (const match of text.matchAll(/\[\s*"([\w.]+)",\s*(?:\(\)|[A-Za-z])/g)) registered.add(match[1]);
}

describe("documented shortcuts (task 242)", () => {
  it("each documented command has a handler", () => {
    expect(COMMANDS.map((command) => command.id).filter((id) => !registered.has(id))).toEqual([]);
  });
});
