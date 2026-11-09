// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from "vitest";
import { createComponent } from "solid-js";
import { render } from "solid-js/web";
import * as accounts from "../../store/accounts";
import { NOW, projected, usageOf } from "../../store/testing";
import { emptyAccounts, type Account, type ProjectedWindow } from "../../types";
import { OverviewScreen } from "./Overview";

vi.mock("../../store/latest", async (original) => ({
  ...await original<object>(), useNow: () => () => 1_793_000_000,
}));

let dispose: (() => void) | undefined;
afterEach(() => { dispose?.(); dispose = undefined; document.body.replaceChildren(); vi.restoreAllMocks(); });

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
