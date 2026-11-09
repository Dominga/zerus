import { beforeEach, describe, expect, it, vi } from "vitest";

// zerus-va84t, zerus-sa9qn.6: what saving an account's appearance writes, on
// its computers and on this desktop, and how this desktop's own marks move.
const state = vi.hoisted(() => ({ writes: [] as [string, unknown][], values: {} as Record<string, unknown> }));
vi.mock("./settings", async (original) => ({
  ...(await original<typeof import("./settings")>()),
  settings: state.values,
  settingsReady: Promise.resolve(),
  setSetting: vi.fn(async (key: string, value: unknown) => {
    state.writes.push([key, value]);
    if (value === null) delete state.values[key]; else state.values[key] = value;
    return true;
  }),
}));

import { profileToken, saveAccountAppearance, type MarkPort } from "./accountMarks";
import { markMigration, RETRY_FIRST, type Schedule } from "./accountMarkMigration";
import { setAccountsPage } from "./accounts";
import { emptyAccounts, type Account, type AccountsView, type Profile } from "../types";
import type { AccountEditResult } from "../bridge";
import { usageOf } from "./testing";

const US = String.fromCharCode(31);
// `hosted` keeps marks on its computer; `older` runs an older hgs.
const hosted = { key: `${US}claude-a1`, host: "", id: "claude-a1", online: true, marks_supported: true, mark_color: "", mark_emoji: "" };
const older = { key: `studio${US}claude-b2`, host: "studio", id: "claude-b2", online: true, marks_supported: false };
const members = [hosted, older];
const account = { key: "account", members } as unknown as Pick<Account, "key" | "members">;
const token = (m: { host: string; id: string }) => profileToken(m.host, m.id);
const writesOf = (prefix: string) => state.writes.filter(([key]) => key.startsWith(prefix));
const hideKeys = members.map((m) => `accounts/hide/${token(m)}`);

function port(result: Partial<AccountEditResult> = {}) {
  const calls: unknown[][] = [];
  const value: MarkPort = {
    open: vi.fn(async (...args: unknown[]) => { calls.push(["open", ...args]); return { handle: "h1" } as never; }),
    apply: vi.fn(async (...args: unknown[]) => {
      calls.push(["apply", ...args]);
      return { notices: [], errors: [], local: [older.key], marked: [hosted.key], ...result };
    }),
  };
  return { value, calls };
}

describe("saving an account's appearance", () => {
  beforeEach(() => { state.writes.length = 0; for (const key of Object.keys(state.values)) delete state.values[key]; });

  it("saves the mark on the computers that keep marks and keeps it here for an older hgs", async () => {
    state.values[`accounts/color/${token(hosted)}`] = "#ff9234";
    const { value, calls } = port();
    expect(await saveAccountAppearance(account, "#2B97FF", " 🦊 ", false, value)).toEqual([]);
    expect(calls).toEqual([["open", "account", hosted.key, "mark"], ["apply", "h1", { color: "#2b97ff", emoji: "🦊" }]]);
    // The computer has it now: this desktop forgets its own for that member.
    expect(writesOf(`accounts/color/${token(hosted)}`)).toEqual([[`accounts/color/${token(hosted)}`, null]]);
    expect(writesOf(`accounts/color/${token(older)}`)).toEqual([[`accounts/color/${token(older)}`, "#2b97ff"]]);
    expect(writesOf(`accounts/mark/${token(older)}`)).toEqual([[`accounts/mark/${token(older)}`, "🦊"]]);
    expect(writesOf("accounts/hide/")).toEqual(hideKeys.map((key) => [key, null]));
  });

  it("hiding stays this desktop's choice for every member", async () => {
    expect(await saveAccountAppearance(account, "#2b97ff", null, true, port().value)).toEqual([]);
    expect(writesOf("accounts/hide/")).toEqual(hideKeys.map((key) => [key, true]));
  });

  it("Reset to default clears the computers' marks and this desktop's", async () => {
    const { value, calls } = port();
    expect(await saveAccountAppearance(account, null, null, false, value)).toEqual([]);
    expect(calls[1]).toEqual(["apply", "h1", { color: "", emoji: "" }]);
    expect(state.writes.every(([, written]) => written === null)).toBe(true);
    expect(state.writes).toHaveLength(members.length * 3);
  });

  it("reports a computer that did not save it and leaves that member's own values", async () => {
    state.values[`accounts/color/${token(hosted)}`] = "#ff9234";
    const { value } = port({ errors: ["workstation is offline."], marked: [] });
    expect(await saveAccountAppearance(account, "#2b97ff", null, false, value)).toEqual(["workstation is offline."]);
    expect(writesOf(`accounts/color/${token(hosted)}`)).toEqual([]);
    const failing: MarkPort = { open: vi.fn(async () => { throw new Error("Accounts are updating."); }), apply: vi.fn() };
    expect(await saveAccountAppearance(account, "#2b97ff", null, false, failing)).toEqual(["Error: Accounts are updating."]);
  });

  it("keeps the mark here without asking when no computer keeps marks", async () => {
    const { value, calls } = port();
    const old = { key: "account", members: [older] } as unknown as Pick<Account, "key" | "members">;
    expect(await saveAccountAppearance(old, "#2b97ff", "🦊", false, value)).toEqual([]);
    expect(calls).toEqual([]);
    expect(writesOf(`accounts/color/${token(older)}`)).toEqual([[`accounts/color/${token(older)}`, "#2b97ff"]]);
  });
});

/** A schedule that never runs: the tests drive each run themselves. */
const never: Schedule = () => () => {};

describe("moving this desktop's marks to their computers (zerus-sa9qn.6)", () => {
  beforeEach(() => { state.writes.length = 0; for (const key of Object.keys(state.values)) delete state.values[key]; });
  const view = (...profiles: Partial<Profile>[]): AccountsView => ({
    ...emptyAccounts(), accounts: [{ key: "account", label: "Work", provider: "claude", members: profiles, usage: usageOf() } as unknown as Account],
  });

  it("moves each mark once, filling only what the computer has no value for, then forgets it here", async () => {
    state.values[`accounts/color/${token(hosted)}`] = "#2B97FF";
    state.values[`accounts/mark/${token(hosted)}`] = "🦊";
    state.values[`accounts/color/${token(older)}`] = "#00aa7f";
    setAccountsPage(view({ ...hosted, mark_color: "#ff9234" }, older));
    const migrate = vi.fn(async () => true);
    await markMigration(migrate, Date.now, never)();
    expect(migrate.mock.calls).toEqual([[hosted.key, "#2b97ff", "🦊"]]);
    expect(state.values).toEqual({ [`accounts/color/${token(older)}`]: "#00aa7f" });
  });

  it("forgets this desktop's values when the computer already has both, and retries an unreachable one later", async () => {
    state.values[`accounts/color/${token(hosted)}`] = "#2b97ff";
    setAccountsPage(view({ ...hosted, mark_color: "#ff9234", mark_emoji: "🐙" }));
    const migrate = vi.fn(async () => true);
    await markMigration(migrate, Date.now, never)();
    expect(migrate).not.toHaveBeenCalled();
    expect(state.values).toEqual({});
    state.values[`accounts/mark/${token(hosted)}`] = "🦊";
    setAccountsPage(view(hosted));
    let now = 1_000, online = false;
    const flaky = vi.fn(async () => online);
    const run = markMigration(flaky, () => now, never);
    await run();
    now += 30_000; await run();
    expect(flaky).toHaveBeenCalledTimes(1);
    expect(state.values).toEqual({ [`accounts/mark/${token(hosted)}`]: "🦊" });
    now += 31_000; online = true; await run();
    expect(flaky.mock.calls[1]).toEqual([hosted.key, "", "🦊"]);
    expect(state.values).toEqual({});
  });

  it("leaves offline members and older computers alone", async () => {
    state.values[`accounts/color/${token(hosted)}`] = "#2b97ff";
    setAccountsPage(view({ ...hosted, online: false }, older));
    const migrate = vi.fn(async () => true);
    await markMigration(migrate, Date.now, never)();
    expect(migrate).not.toHaveBeenCalled();
    expect(state.values).toEqual({ [`accounts/color/${token(hosted)}`]: "#2b97ff" });
  });

  it("schedules its own retry, backing off, and stops once moved or no longer pending (zerus-k4odl)", async () => {
    state.values[`accounts/mark/${token(hosted)}`] = "🦊";
    setAccountsPage(view(hosted));
    let now = 0, online = false;
    const timers: { run: () => void; ms: number; cancelled: boolean }[] = [];
    const schedule: Schedule = (run, ms) => { const t = { run, ms, cancelled: false }; timers.push(t); return () => { t.cancelled = true; }; };
    const flaky = vi.fn(async () => online);
    const settle = () => new Promise((resolve) => setTimeout(resolve, 0));
    await markMigration(flaky, () => now, schedule)();
    expect(timers.map((t) => t.ms)).toEqual([RETRY_FIRST]);
    // No Accounts change: the timer alone tries again, then waits twice as long.
    now += RETRY_FIRST; timers[0].run(); await settle();
    expect(flaky).toHaveBeenCalledTimes(2);
    expect(timers.map((t) => t.ms)).toEqual([RETRY_FIRST, 2 * RETRY_FIRST]);
    online = true; now += 2 * RETRY_FIRST; timers[1].run(); await settle();
    expect(flaky).toHaveBeenCalledTimes(3);
    expect(state.values).toEqual({});
    expect(timers).toHaveLength(2);
    // A member that is no longer pending is forgotten with its timer.
    state.values[`accounts/mark/${token(hosted)}`] = "🦊";
    online = false;
    const run = markMigration(flaky, () => now, schedule);
    await run();
    expect(timers).toHaveLength(3);
    setAccountsPage(view({ ...hosted, online: false }));
    await run();
    expect(timers[2].cancelled).toBe(true);
    expect(timers).toHaveLength(3);
  });
});
