import { beforeEach, describe, expect, it } from "vitest";
import {
  closeWorkingMode, markWorkingModeKept, resetWorkingMode, workingModeClosed, workingModeFingerprint, workingModeShown,
} from "./workingMode";
import type { TeamView } from "../types";

// Task #337: the working-mode block stays away after the owner keeps the
// settings (hgs records that) or closes it for this session of the profile,
// and comes back only when the project's profile changes materially.
function team(fingerprint = "10:20:8:2", disagreeing = ["folder policy"]): TeamView {
  return {
    id: "t1", name: "Checkout", revision: 4,
    working_mode: {
      preset: "branch-pipeline",
      proposal: { preset: "branch-pipeline", title: "Branch pipeline", reasons: ["A code repository"], fingerprint },
      disagreeing,
    },
  } as unknown as TeamView;
}

beforeEach(() => resetWorkingMode());

describe("the working-mode block's local state (task #337)", () => {
  it("shows while the project's profile disagrees, and a close hides it for that profile", () => {
    const one = team();
    expect(workingModeShown(one)).toBe(true);
    closeWorkingMode(one);
    expect(workingModeClosed(one)).toBe(true);
    expect(workingModeShown(one)).toBe(false);
    // The same profile never brings it back, even after a fresh team read.
    expect(workingModeShown(team())).toBe(false);
    // A materially changed profile is another fingerprint: the block returns.
    const changed = team("11:21:9:2");
    expect(workingModeFingerprint(changed)).toBe("11:21:9:2");
    expect(workingModeShown(changed)).toBe(true);
  });

  it("stays away once the owner keeps the settings, until the profile changes", () => {
    const one = team();
    markWorkingModeKept(one);
    expect(workingModeShown(one)).toBe(false);
    expect(workingModeShown(team())).toBe(false);
    expect(workingModeShown(team("11:21:9:2"))).toBe(true);
    // The record hgs sends after the keep has the same effect on a fresh launch.
    resetWorkingMode();
    const kept = team();
    kept.working_mode!.kept = { preset: "branch-pipeline", fingerprint: "10:20:8:2", at: 1 };
    expect(workingModeShown(kept)).toBe(false);
    expect(workingModeShown(team("11:21:9:2"))).toBe(true);
  });

  it("never shows without a disagreement", () => {
    expect(workingModeShown(team("10:20:8:2", []))).toBe(false);
    expect(workingModeShown({ id: "t2", name: "No mode" } as TeamView)).toBe(false);
  });
});
