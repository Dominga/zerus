import { describe, expect, it } from "vitest";
import { createRoot } from "solid-js";
import { SHARE, SIDE, createTeamLayout, dragShare, dragSide, teamLayout } from "./teamLayout";
import { DEFAULTS, num, settingsReady } from "./settings";

// Task 9: the Team tab's dividers are the owner's. A drag previews, letting go
// stores once, a lost drag keeps the stored size, and nothing else moves them.

function memory(initial: Record<string, number> = {}) {
  const values = new Map(Object.entries(initial));
  const writes: [string, number][] = [];
  return {
    writes,
    store: { read: (key: string) => values.get(key) ?? Number.NaN, write: (key: string, value: number) => { writes.push([key, value]); values.set(key, value); } },
  };
}

describe("Team tab dividers", () => {
  it("start at the defaults and keep a stored choice within its bounds", () => {
    createRoot((dispose) => {
      expect(createTeamLayout(memory().store).sideWidth()).toBe(SIDE.initial);
      expect(createTeamLayout(memory().store).membersShare()).toBe(SHARE.initial);
      const stored = createTeamLayout(memory({ [SIDE.key]: 2000, [SHARE.key]: 3 }).store);
      expect([stored.sideWidth(), stored.membersShare()]).toEqual([SIDE.max, SHARE.min]);
      dispose();
    });
  });

  it("store a drag once, when the pointer lets go, and only a changed value", () => {
    createRoot((dispose) => {
      const { store, writes } = memory({ [SHARE.key]: 45 });
      const layout = createTeamLayout(store);
      layout.previewShare(60);
      layout.previewShare(62);
      expect(layout.membersShare()).toBe(62);
      expect(writes).toEqual([]);
      layout.commitShare();
      expect(writes).toEqual([[SHARE.key, 62]]);
      expect(layout.membersShare()).toBe(62);
      // A click without movement, or the same value, stores nothing.
      layout.commitShare();
      layout.previewShare(62);
      layout.commitShare();
      expect(writes).toHaveLength(1);
      dispose();
    });
  });

  it("keep the stored size when a drag is cut off, and return to the default on a double click", () => {
    createRoot((dispose) => {
      const { store, writes } = memory({ [SIDE.key]: 500 });
      const layout = createTeamLayout(store);
      layout.previewSide(700);
      layout.cancel();
      expect(layout.sideWidth()).toBe(500);
      expect(writes).toEqual([]);
      layout.resetSide();
      expect(writes).toEqual([[SIDE.key, SIDE.initial]]);
      layout.stepShare(1);
      layout.stepShare(-1);
      expect(writes.slice(1)).toEqual([[SHARE.key, SHARE.initial + SHARE.step], [SHARE.key, SHARE.initial]]);
      dispose();
    });
  });

  it("turn a pointer's move into a share of the column and a side width", () => {
    expect(dragShare(45, 100, 160, 600)).toBe(55);
    expect(dragShare(45, 100, -2000, 600)).toBe(SHARE.min);
    expect(dragShare(45, 100, 160, 0)).toBe(45);
    // The side column grows as the pointer moves left; the canvas keeps a third of the tab.
    expect(dragSide(420, 1000, 900, 1600)).toBe(520);
    expect(dragSide(420, 1000, 100, 1200)).toBe(800);
    expect(dragSide(420, 1000, 1500, 1600)).toBe(SIDE.min);
  });

  it("persist in the client's settings, so a restart keeps the owner's choice", async () => {
    await settingsReady;
    expect([DEFAULTS[SIDE.key], DEFAULTS[SHARE.key]]).toEqual([SIDE.initial, SHARE.initial]);
    teamLayout.previewShare(30);
    teamLayout.commitShare();
    teamLayout.previewSide(600);
    teamLayout.commitSide();
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect([num(SHARE.key), num(SIDE.key)]).toEqual([30, 600]);
    expect([teamLayout.membersShare(), teamLayout.sideWidth()]).toEqual([30, 600]);
  });
});
