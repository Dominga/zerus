import { beforeEach, describe, expect, it, vi } from "vitest";
import { createSignal } from "solid-js";
import * as bridge from "../bridge";
import { screenOf, startScreens } from "./screens";
import { latest } from "./latest";
import { emptyAccounts, emptyMachines, emptyProjects } from "../types";
import type { Page } from "./ui";

vi.mock("../bridge", () => ({
  screenVisible: vi.fn(async () => {}),
  onScreensChanged: vi.fn(() => () => {}),
  accountsView: vi.fn(async () => emptyAccounts()),
  machinesView: vi.fn(async () => emptyMachines()),
  machineResolve: vi.fn(async () => emptyMachines()),
  machineCheck: vi.fn(async () => emptyMachines()),
  projectsView: vi.fn(async () => emptyProjects()),
}));
const mocked = vi.mocked(bridge);
const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

describe("screens wiring", () => {
  beforeEach(() => vi.clearAllMocks());

  it("maps pages to screens and hides them with the window", () => {
    expect(screenOf("dashboard", false)).toBe("overview");
    expect(screenOf("machines", false)).toBe("machines");
    expect(screenOf("sessions", false)).toBeUndefined();
    expect(screenOf("accounts", true)).toBeUndefined();
  });

  it("tells the hub which screen shows and pulls it", async () => {
    let hidden = false;
    const listeners: (() => void)[] = [];
    const [page, setPage] = createSignal<Page>("sessions");
    const stop = startScreens(page, { hidden: () => hidden, onChange: (l) => { listeners.push(l); return () => {}; } });
    setPage("dashboard");
    await settle();
    expect(mocked.screenVisible.mock.calls).toEqual([["overview", true]]);
    expect(mocked.accountsView).toHaveBeenCalledTimes(1);
    setPage("machines");
    await settle();
    expect(mocked.screenVisible.mock.calls.slice(1)).toEqual([["machines", true], ["overview", false]]);
    expect(mocked.machinesView).toHaveBeenCalledWith(false);
    hidden = true;
    listeners.forEach((l) => l());
    await settle();
    expect(mocked.screenVisible.mock.calls.at(-1)).toEqual(["machines", false]);
    stop();
  });

  it("shows the new screen before it hides the old one, one change at a time", async () => {
    const answers: (() => void)[] = [];
    mocked.screenVisible.mockImplementation(() => new Promise<void>((resolve) => answers.push(resolve)));
    try {
      const [page, setPage] = createSignal<Page>("dashboard");
      const stop = startScreens(page, { hidden: () => false, onChange: () => () => {} });
      await settle();
      answers.shift()!();
      await settle();
      expect(mocked.accountsView).toHaveBeenCalledTimes(1);
      setPage("accounts");
      await settle();
      // Accounts shows first, so the hub never sees neither account screen and keeps its queued inspects.
      expect(mocked.screenVisible.mock.calls.slice(1)).toEqual([["accounts", true]]);
      expect(mocked.accountsView).toHaveBeenCalledTimes(1);
      answers.shift()!();
      await settle();
      expect(mocked.screenVisible.mock.calls.slice(1)).toEqual([["accounts", true], ["overview", false]]);
      expect(mocked.accountsView).toHaveBeenCalledTimes(2);
      answers.shift()!();
      stop();
    } finally {
      mocked.screenVisible.mockImplementation(async () => {});
    }
  });

  it("keeps sending visibility changes after one call rejected", async () => {
    mocked.screenVisible.mockRejectedValueOnce(new Error("hub gone"));
    const [page, setPage] = createSignal<Page>("sessions");
    const stop = startScreens(page, { hidden: () => false, onChange: () => () => {} });
    setPage("projects");
    await settle();
    setPage("machines");
    await settle();
    expect(mocked.screenVisible.mock.calls).toEqual([["projects", true], ["machines", true], ["projects", false]]);
    stop();
  });

  it("re-pulls the visible screen on screens-changed", async () => {
    let fire: (screen: "projects" | "accounts") => void = () => {};
    mocked.onScreensChanged.mockImplementation((listener) => { fire = listener; return () => {}; });
    const [page] = createSignal<Page>("projects");
    const stop = startScreens(page, { hidden: () => false, onChange: () => () => {} });
    await settle();
    mocked.projectsView.mockClear();
    fire("accounts");
    fire("projects");
    await settle();
    expect(mocked.projectsView).toHaveBeenCalledTimes(1);
    expect(mocked.accountsView).not.toHaveBeenCalled();
    stop();
  });

  it("applies only the answer to the latest pull", async () => {
    const got: number[] = [];
    const pull = latest<number>((value) => got.push(value));
    let first!: (n: number) => void;
    const slow = pull(() => new Promise<number>((resolve) => { first = resolve; }));
    await pull(async () => 2);
    first(1);
    await slow;
    expect(got).toEqual([2]);
  });
});
