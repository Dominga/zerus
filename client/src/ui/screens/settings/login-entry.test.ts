import { describe, expect, it, vi } from "vitest";
import { createLoginEntry, loginEntryHint, loginEntryMobileLine, type LoginEntry, type LoginState } from "./login-entry";

const entry = (state: LoginState, available = true): LoginEntry => ({ available, state });
const settle = () => new Promise((resolve) => setTimeout(resolve));

function fixture(first: LoginEntry = entry("off")) {
  const read = vi.fn(async () => first);
  const write = vi.fn(async (enabled: boolean) => entry(enabled ? "on" : "off"));
  return { read, write, store: createLoginEntry({ read, write }) };
}

describe("Start Zerus at login", () => {
  it("shows only what the native read says and writes nothing when it reads", async () => {
    const t = fixture(entry("on"));
    expect(t.store.available()).toBe(false);
    await t.store.load();
    expect(t.store.available()).toBe(true);
    expect(t.store.checked()).toBe(true);
    expect(t.write).not.toHaveBeenCalled();
  });
  it("is hidden where the native gate refuses it", async () => {
    const t = fixture(entry("unknown", false));
    await t.store.load();
    expect(t.store.available()).toBe(false);
  });
  it("is hidden when the read itself fails", async () => {
    const t = fixture();
    t.read.mockRejectedValueOnce(new Error("no native side"));
    await t.store.load();
    expect(t.store.available()).toBe(false);
  });
  it.each<[LoginState, boolean, boolean]>([
    ["on", true, false],
    ["off", false, false],
    ["other", false, false],
    ["unknown", false, true],
  ])("%s is checked %s and disabled %s", async (state, checked, disabled) => {
    const t = fixture(entry(state));
    await t.store.load();
    expect(t.store.checked()).toBe(checked);
    expect(t.store.disabled()).toBe(disabled);
  });
  it("sends only the person's on or off, once, and shows the native answer", async () => {
    const t = fixture(entry("off"));
    await t.store.load();
    const first = t.store.choose(true);
    expect(t.store.disabled()).toBe(true);
    await t.store.choose(true);
    await first;
    expect(t.write.mock.calls).toEqual([[true]]);
    expect(t.store.checked()).toBe(true);
    expect(t.store.disabled()).toBe(false);
    await t.store.choose(false);
    expect(t.write.mock.calls).toEqual([[true], [false]]);
    expect(t.store.checked()).toBe(false);
  });
  it("shows a refused write and reads the real state again", async () => {
    const t = fixture(entry("off"));
    await t.store.load();
    t.write.mockRejectedValueOnce("Cannot change the per-user login registration (5)");
    t.read.mockResolvedValueOnce(entry("unknown"));
    await t.store.choose(true);
    expect(t.store.error()).toBe("Cannot change the per-user login registration (5)");
    expect(t.store.state()).toBe("unknown");
    await t.store.load();
    expect(t.store.error()).toBe("");
  });
  it("an older read cannot replace a newer answer", async () => {
    const t = fixture(entry("off"));
    let finish!: (value: LoginEntry) => void;
    t.read.mockImplementationOnce(() => new Promise<LoginEntry>((resolve) => { finish = resolve; }));
    const reading = t.store.load();
    await settle();
    await t.store.choose(true);
    finish(entry("off"));
    await reading;
    expect(t.store.checked()).toBe(true);
  });
  it("says it neither sees nor removes a Startup shortcut in every state", () => {
    for (const state of ["on", "off", "other", "unknown"] as LoginState[]) {
      expect(loginEntryHint(state)).toMatch(/doesn't detect or remove a shortcut/);
    }
    expect(loginEntryHint("off")).not.toMatch(/another copy/);
    expect(loginEntryHint("other")).toMatch(/another copy/);
    expect(loginEntryHint("unknown")).toMatch(/can't read/);
  });
  it("the Mobile line allows for a shortcut it cannot see and never claims a confirmed off", () => {
    expect(loginEntryMobileLine("on")).toMatch(/starts at login/);
    expect(loginEntryMobileLine("off")).toMatch(/unless a Startup shortcut/);
    expect(loginEntryMobileLine("other")).toMatch(/can't confirm/);
    expect(loginEntryMobileLine("unknown")).toMatch(/can't confirm/);
  });
});
