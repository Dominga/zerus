import { describe, expect, it } from "vitest";
import { emptyUpdatesView, type ForkUpdatesView, type UpdatesView } from "../../types";
import { titleUpdate } from "./title-update";

const fork = (over: Partial<ForkUpdatesView> = {}): ForkUpdatesView => ({
  supported: true, automatic: true, repository: "WhiteWh/zerus", installed_build: 191, running_commit: "",
  status: { source: "" }, busy: false, offered_build: 0, offered_commit: "", ready_build: 0, installing: false, newer_build: 0, ...over,
});
const view = (over: Partial<UpdatesView> = {}): UpdatesView => ({ ...emptyUpdatesView(), ...over });

describe("the title bar's update button (owner, 2026-11-13)", () => {
  it("follows the fork's feed in a fork build", () => {
    expect(titleUpdate(view({ fork: fork() }))).toEqual({ kind: "none" });
    expect(titleUpdate(view({ fork: fork({ newer_build: 195, busy: true }) }))).toEqual({ kind: "available", build: 195 });
    expect(titleUpdate(view({ fork: fork({ offered_build: 163, installed_build: 0 }) }))).toEqual({ kind: "available", build: 163 });
    // A verified build waits: Restart comes first.
    expect(titleUpdate(view({ fork: fork({ newer_build: 196, ready_build: 195 }), restart_required: true }))).toEqual({ kind: "restart", build: 195 });
  });

  it("follows upstream's channel otherwise, unless its prompts are quiet", () => {
    expect(titleUpdate(view({ available: true, version: "0.50.0" }))).toEqual({ kind: "available", version: "0.50.0" });
    expect(titleUpdate(view({ available: true, restart_required: true }))).toEqual({ kind: "restart" });
    expect(titleUpdate(view({ available: true, quiet: true }))).toEqual({ kind: "none" });
    expect(titleUpdate(view())).toEqual({ kind: "none" });
  });
});
