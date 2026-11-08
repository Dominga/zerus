import { describe, expect, it } from "vitest";
import source from "./SessionDetail.tsx?raw";

// Task #331: the strip must not use a fresh tabs() array as its For source.
// That array is rebuilt by every status poll, team read and unread change; the
// old objects made Solid recreate each button, restarting .tab.on::after's
// grow animation on the active tab without a click. The fixed ids below keep
// each button mounted and let only a detailTab change toggle "on".
describe("session tab strip stability", () => {
  it("keeps one button per tab id, so a data update cannot remount the underline", () => {
    expect(source).toContain('export const TAB_ORDER = ["activity", "terminal", "native", "processes", "taskview", "team", "newteam"] as const;');
    expect(source).toContain("<For each={TAB_ORDER}>");
    expect(source).not.toMatch(/<For\s+each=\{tabs\(\)/);
  });
});