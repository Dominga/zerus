import { describe, expect, it } from "vitest";
import { memberStateColor, memberStatus, progress, stateLabel, stateTone } from "./team-model";
import { teamView } from "./fixtures";

// The Team tab's vocabulary from the core's view: state words and tones, a
// member's state and the bar's counts. No phases or task timings (M2).
describe("the team's vocabulary", () => {
  it("labels and tones a team's status", () => {
    expect(stateLabel("running")).toBe("running");
    expect(stateTone("running")).toBe("active");
    expect(stateTone("paused")).toBe("warn");
    expect(stateTone("stopped")).toBe("danger");
    expect(stateTone("finished")).toBe("done");
    expect(stateTone("archived")).toBe("muted");
  });

  it("gives a member's state a word and a colour", () => {
    expect(memberStateColor("active")).toBe("info");
    expect(memberStateColor("paused")).toBe("warn");
    expect(memberStateColor("removed")).toBe("danger");
    expect(memberStateColor("not_created")).toBe("dim");
    const status = memberStatus(teamView.members[2]);
    expect(status.label).toBe("not created");
    expect(status.lead).toBe(false);
  });

  it("counts the bar from the core's counts", () => {
    expect(progress(teamView)).toEqual({ accepted: 0, reported: 1, active: 0, waiting: 1, total: 2 });
  });
});

it("excludes cancelled nodes from the Team tab total", () => {
  // Negative UI control over the pinned core counts; no Teams rows are seeded.
  const view = { ...teamView, bar: { ...teamView.bar, counts: { ...teamView.bar.counts, nodes: { ...teamView.bar.counts.nodes, cancelled: 9 } } } };
  expect(progress(view)).toEqual(progress(teamView));
});
