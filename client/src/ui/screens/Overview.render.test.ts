// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from "vitest";
import { createComponent } from "solid-js";
import { render } from "solid-js/web";
import * as accounts from "../../store/accounts";
import { NOW, machineMeta, projected, rowOut, usageOf } from "../../store/testing";
import { applyBatch } from "../../store/catalog";
import { restoreSettings, setSetting } from "../../store/settings";
import { emptyAccounts, emptyBatch, type Account, type ProjectedWindow, type ViewRow } from "../../types";
import { OverviewScreen } from "./Overview";

vi.mock("../../store/latest", async (original) => ({
  ...await original<object>(), useNow: () => () => 1_793_000_000,
}));
// Layout is not under test: keep real Overview/SessionCard rendering while
// exposing each grid line in happy-dom's zero-sized viewport.
vi.mock("@tanstack/solid-virtual", () => ({ createVirtualizer: (options: { count: number }) => ({
  getTotalSize: () => options.count * 112,
  getVirtualItems: () => Array.from({ length: options.count }, (_, index) => ({ index, start: index * 112 })),
}) }));

let dispose: (() => void) | undefined;
afterEach(() => {
  dispose?.(); dispose = undefined; document.body.replaceChildren(); vi.restoreAllMocks();
  applyBatch({ ...emptyBatch(), replaces: [{ computer: "", rows: [] }] }); restoreSettings();
});

describe("Overview Main only catalog parents", () => {
  const machine = "11111111-1111-4111-8111-111111111111";
  const parentId = "00000000-0000-4000-8000-000000000001";

  it.each([
    ["idle", { status: "ready", phase: "idle" }, false],
    ["usage-limited", { status: "error", phase: "error", summary: "Weekly usage limit reached", needs_attention: false }, false],
    ["archived", { status: "archived", state: "archived", archive_id: "synthetic-archive" }, true],
    ["missing", undefined, true],
  ] as const)("filters a child with a %s parent, and leaves With sub-sessions unchanged", async (_, parent, shown) => {
    await setSetting("client/overviewSubSessions", false, { persist: false });
    const child = rowOut("", 2, { title: "Synthetic child", status: "working", parent: {
      machine, session_id: parentId, conversation_id: "parent-conversation", run_id: "parent-run", name: "claude/sample-project/t1", state: "live",
    } });
    const rows = parent ? [rowOut("", 1, { session_id: parentId, needs_attention: false, ...parent } as Partial<ViewRow>), child] : [child];
    applyBatch({ ...emptyBatch(), computers: [{ computer: "", status: "live", attempt: 0 }],
      machines: [{ computer: "", machine: machineMeta("workstation", {}, { machine_id: machine }) }],
      replaces: [{ computer: "", rows }] });
    vi.spyOn(accounts, "accountsPage").mockReturnValue(emptyAccounts());
    const host = document.createElement("div"); document.body.append(host);
    dispose = render(() => createComponent(OverviewScreen, {}), host);
    expect(host.querySelectorAll(".ov-cell")).toHaveLength(shown ? 1 : 0);
    expect(host.querySelector(".ov-cell")?.textContent ?? "").not.toContain("Weekly usage limit reached");
    const toggle = [...host.querySelectorAll("button")].find((b) => b.textContent === "With sub-sessions")!;
    toggle.click();
    await Promise.resolve();
    expect(toggle.getAttribute("aria-pressed")).toBe("true");
    expect(host.querySelectorAll(".ov-cell")).toHaveLength(1);
    expect(host.querySelector(".ov-cell")?.textContent).toContain("Synthetic child");
  });
});

function show(windows: ProjectedWindow[]) {
  const account: Account = { key: "fixture", label: "Work", provider: "claude", installed: true, machines: [], members: [],
    usage: usageOf({ windows }), permission: { source: "Default" }, identity_lines: [], identity_note: null };
  vi.spyOn(accounts, "accountsPage").mockReturnValue({ ...emptyAccounts(), snapshot: true, overview: [account] });
  const host = document.createElement("div"); document.body.append(host);
  dispose = render(() => createComponent(OverviewScreen, {}), host);
  return host.querySelector(".ov-acard")?.textContent;
}

describe("dashboard account reset countdowns", () => {
  it.each([0, 34, 100])("shows the reset time at %s percent used", (used) => {
    expect(show([projected({ used, resets_at: NOW + 7200 })])).toContain(`5h ${used}% (2h 0m)`);
  });

  it("shows each displayed limit's own reset and does not invent an unknown reset", () => {
    const text = show([projected({ used: 34, resets_at: NOW + 7200 }), projected({ id: "week", minutes: 10080, used: 12, resets_at: null })]);
    expect(text).toContain("5h 34% (2h 0m)");
    expect(text).toContain("7d 12% (reset unknown)");
  });

  it("asks for refreshed usage once a window expires", () => {
    const text = show([projected({ used: 34, resets_at: NOW - 60 })]);
    expect(text).toContain("5h refresh");
    expect(text).not.toContain("2h 0m");
  });
});

describe("dashboard DeepSeek API-key balance", () => {
  it("shows the Total as a symbol amount and names it in full on hover", () => {
    const usage = usageOf({ provider: "dsh", status: "configured", balances: [
      { kind: "Total", balance: "6.20", currency: "USD" }, { kind: "Topped up", balance: "6.20", currency: "USD" }, { kind: "Granted", balance: "0.00", currency: "USD" }] });
    const account: Account = { key: "dsh", label: "DeepSeek API", provider: "dsh", installed: true, machines: [], members: [],
      usage, permission: { source: "Default" }, identity_lines: [], identity_note: null };
    vi.spyOn(accounts, "accountsPage").mockReturnValue({ ...emptyAccounts(), snapshot: true, overview: [account] });
    const host = document.createElement("div"); document.body.append(host);
    dispose = render(() => createComponent(OverviewScreen, {}), host);
    const summary = host.querySelector(".ov-acard .um-summary");
    expect(summary?.querySelector("[aria-hidden='true']")?.textContent).toBe("$6.20");
    expect(summary?.getAttribute("title")).toBe("Balance: 6.20 USD\nTopped up: 6.20 USD\nGranted: 0.00 USD");
    expect(summary?.textContent).not.toContain("Total");
  });
});
