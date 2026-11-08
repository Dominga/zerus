import { describe, expect, it } from "vitest";
import { COUNTS, createRuleCounts, ironRulesChip } from "./ironRuleCounts";
import type { RulesCall, RulesOutcome } from "../bridge";

// Qt parity (I5b): the composer's iron rules chip and the count it shows.

const tick = () => new Promise((resolve) => setTimeout(resolve, 0));
const done = (rules: string[]): RulesOutcome => ({ outcome: "done", view: { rules, revision: 1 } });

describe("the iron rules chip", () => {
  it("names one rule or many and hides without any", () => {
    expect(ironRulesChip(0)).toBe("");
    expect(ironRulesChip(1)).toBe("1 iron rule");
    expect(ironRulesChip(3)).toBe("3 iron rules");
  });

  it("keeps what the Iron rules card reports, per session and bounded", () => {
    const counts = createRuleCounts(async () => done([]));
    expect([counts.count("a"), counts.known("a")]).toEqual([0, false]);
    counts.set("a", 2);
    counts.set("b", 0);
    expect([counts.count("a"), counts.known("a"), counts.count("b"), counts.known("b")]).toEqual([2, true, 0, true]);
    for (let n = 0; n < COUNTS; n++) counts.set(`s${n}`, 1);
    expect(counts.known("a")).toBe(false);
    expect(counts.known(`s${COUNTS - 1}`)).toBe(true);
  });

  it("reads an unreported session's project rules once", async () => {
    const calls: RulesCall[] = [];
    const counts = createRuleCounts(async (call) => { calls.push(call); return done(["Keep tests green", "Ask before pushing"]); });
    await counts.ensure("studio\nrun:run-1", "studio", "orbit");
    await counts.ensure("studio\nrun:run-1", "studio", "orbit");
    expect(calls).toEqual([{ computer: "studio", target: { scope: "project", project: "orbit" }, op: { op: "read" } }]);
    expect(counts.count("studio\nrun:run-1")).toBe(2);
    // No project, or a count the card already gave: nothing is read.
    await counts.ensure("\nrun:run-2", "", "");
    counts.set("\nrun:run-3", 4);
    await counts.ensure("\nrun:run-3", "", "orbit");
    expect(calls).toHaveLength(1);
    expect(counts.count("\nrun:run-3")).toBe(4);
  });

  it("lets the card's count win and leaves a failed read unknown", async () => {
    let answer!: (outcome: RulesOutcome) => void;
    const counts = createRuleCounts(() => new Promise((resolve) => { answer = resolve; }));
    const read = counts.ensure("a", "", "orbit");
    counts.set("a", 5);
    answer(done(["One"]));
    await read;
    expect(counts.count("a")).toBe(5);

    const failing = createRuleCounts(async () => { throw new Error("offline"); });
    await failing.ensure("b", "", "orbit");
    expect(failing.known("b")).toBe(false);
    const refused = createRuleCounts(async () => ({ outcome: "failed", detail: { source: "No access" } }));
    await refused.ensure("c", "", "orbit");
    await tick();
    expect([refused.known("c"), refused.count("c")]).toEqual([false, 0]);
  });
});
