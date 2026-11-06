import { describe, expect, it } from "vitest";
import { forkActions, forkInstalled } from "./forkUpdates";
import type { ForkUpdatesView } from "../../../types";

// WhiteWh fork, task #63: the fork channel's buttons in Settings > Updates.

function view(over: Partial<ForkUpdatesView> = {}): ForkUpdatesView {
  return {
    supported: true, automatic: true, repository: "WhiteWh/zerus", installed_build: 41, running_commit: "0123456789abcdef0123456789abcdef01234567",
    status: { source: "Fork build %1 is the newest.", args: [{ kind: "number", value: 41 }] }, busy: false,
    offered_build: 0, offered_commit: "", ready_build: 0, installing: false, ...over,
  };
}

describe("fork updates", () => {
  it("checks when idle, offers a replacement and a restart", () => {
    expect(forkActions(view())).toEqual({ check: true, replace: 0, restart: 0 });
    expect(forkActions(view({ busy: true }))).toEqual({ check: false, replace: 0, restart: 0 });
    expect(forkActions(view({ offered_build: 44 })).replace).toBe(44);
    expect(forkActions(view({ ready_build: 42 })).restart).toBe(42);
    expect(forkActions(view({ ready_build: 42, installing: true }))).toEqual({ check: false, replace: 0, restart: 0 });
  });

  it("offers nothing outside a portable fork installation", () => {
    expect(forkActions(null)).toEqual({ check: false, replace: 0, restart: 0 });
    expect(forkActions(view({ supported: false, ready_build: 42 }))).toEqual({ check: false, replace: 0, restart: 0 });
  });

  it("names the running build by its number, else its commit", () => {
    expect(forkInstalled(view())).toEqual({ build: 41, commit: "0123456789" });
    expect(forkInstalled(view({ installed_build: 0 })).build).toBe(0);
  });
});
