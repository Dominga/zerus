import { describe, expect, it } from "vitest";
import type { TeamMemberView } from "../../types";
import { memberIronUpdates, settingsPatch, type SettingsForm } from "./teamSettings";

const settings: Record<string, unknown> = {
  iron_rules: ["Keep it small"],
  branch_per_task: { template: "task/{id}" },
  base_branch: "main",
  restart_crashed: false,
  land_allowed: false,
  build_broker: false,
  lead_removes_own_pause: false,
  phase_acceptance: "lead",
};

const form = (changes: Partial<SettingsForm> = {}): SettingsForm => ({
  ironRules: "Keep it small",
  branchOn: true,
  template: "task/{id}",
  base: "main",
  restart: false,
  land: false,
  broker: false,
  leadUnpause: false,
  acceptance: false,
  ...changes,
});

const member = (rules: string[]): TeamMemberView =>
  ({ id: "dev", role: "Backend", agent: "claude", state: "active", iron_rules: rules }) as TeamMemberView;

describe("team settings round-trip", () => {
  it("sends only changed settings, with Qt's branch object and null", () => {
    expect(settingsPatch(settings, form())).toEqual({});
    const patch = settingsPatch(settings, form({
      ironRules: "Keep it small\nRun the tests",
      template: "task/{member}/{id}",
      base: "next",
      land: true,
      broker: true,
      restart: true,
      leadUnpause: true,
      acceptance: true,
    }));
    expect(patch).toEqual({
      iron_rules: ["Keep it small", "Run the tests"],
      branch_per_task: { template: "task/{member}/{id}" },
      base_branch: "next",
      restart_crashed: true,
      land_allowed: true,
      build_broker: true,
      lead_removes_own_pause: true,
      phase_acceptance: "owner",
    });
    expect(settingsPatch(settings, form({ branchOn: false }))).toEqual({ branch_per_task: null });
  });

  it("sends each changed member's iron rules only", () => {
    expect(memberIronUpdates([member(["old"])], { dev: "old" })).toEqual([]);
    expect(memberIronUpdates([member(["old"])], { dev: "old\nnew" })).toEqual([
      { id: "dev", iron_rules: ["old", "new"] },
    ]);
  });
});
