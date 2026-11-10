import { describe, expect, it } from "vitest";
import { fallbackTab } from "./fallbackTab";

describe("the tab shown when the current one hides", () => {
  const tabs = (shown: Record<string, boolean>) =>
    (["activity", "terminal", "team", "newteam"] as const).map((id) => ({ id, show: shown[id] ?? id === "activity" }));

  it("keeps a tab that still shows", () => {
    expect(fallbackTab(tabs({ newteam: true }), "newteam")).toBeUndefined();
  });

  it("follows a session that just got its team from + New team to its Team tab", () => {
    // Owner, 2026-11-10: the first press of Start planning or Create team used to land on Activity.
    expect(fallbackTab(tabs({ team: true }), "newteam")).toBe("team");
  });

  it("returns to Activity when + New team hides without a team to show", () => {
    expect(fallbackTab(tabs({}), "newteam")).toBe("activity");
  });

  it("returns any other hidden tab to Activity", () => {
    expect(fallbackTab(tabs({ team: true }), "terminal")).toBe("activity");
  });
});
