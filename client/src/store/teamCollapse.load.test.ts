import { describe, expect, it, vi } from "vitest";

// zerus-xmjgp: a choice made while the stored values load joins them.
const MACHINE = "11111111-1111-4111-8111-111111111111";
const A = `${MACHINE}\n22222222-2222-4222-8222-222222222222`;
const B_SESSION = "33333333-3333-4333-8333-333333333333";
const store = vi.hoisted(() => {
  let resolve!: () => void;
  const ready = new Promise<void>((done) => { resolve = done; });
  return { settings: {} as Record<string, unknown>, writes: [] as [string, unknown][], ready, resolve: () => resolve() };
});
vi.mock("./settings", () => ({
  settings: store.settings,
  settingsReady: store.ready,
  setSetting: vi.fn(async (key: string, value: unknown) => { store.writes.push([key, value]); store.settings[key] = value; return true; }),
}));
vi.mock("./catalog", () => ({ catalog: { rows: { "\nrun:b": { computer: "", sessionId: "33333333-3333-4333-8333-333333333333" } } } }));
vi.mock("./machines", () => ({ machineCards: { "": { machine_id: "11111111-1111-4111-8111-111111111111" } } }));

import { COLLAPSED_TEAMS, setTeamCollapsed, teamCollapsed } from "./teamCollapse";

describe("collapsed parents while the settings load", () => {
  it("keeps the saved parents and adds the new choice", async () => {
    setTeamCollapsed("\nrun:b", true);
    // The window shows the choice at once; nothing is written yet.
    expect(teamCollapsed("\nrun:b")).toBe(true);
    expect(store.writes).toEqual([]);
    // The stored values arrive with parent A collapsed.
    store.settings[COLLAPSED_TEAMS] = JSON.stringify([A]);
    store.resolve();
    await store.ready; await Promise.resolve(); await Promise.resolve();
    expect(store.writes).toEqual([[COLLAPSED_TEAMS, JSON.stringify([A, `${MACHINE}\n${B_SESSION}`])]]);
  });
});
