import { describe, expect, it } from "vitest";
import { createRoot } from "solid-js";
import { createIronRules, MAX_RULES } from "./ironRules";
import type { RulesCall, RulesOutcome } from "../bridge";

// Task 68: Qt's IronRulesPanel over client/core's guarded `hgs rules` calls.

const tick = () => new Promise((resolve) => setTimeout(resolve, 0));
const done = (rules: string[], revision: number): RulesOutcome => ({ outcome: "done", view: { rules, revision } });
const session = { name: "claude/app/main", run_id: "run-1", conversation_id: "conv-1" };

function fixture(answers: (call: RulesCall) => RulesOutcome, confirm = true) {
  const calls: RulesCall[] = [];
  const asked: [string, number][] = [];
  const rules = createRoot(() => createIronRules({
    call: async (call) => { calls.push(call); return answers(call); },
    confirmRemove: async (rule, number) => { asked.push([rule, number]); return confirm; },
  }));
  return { rules, calls, asked };
}

describe("iron rules", () => {
  it("reads the project's rules on this computer and the session's additions with its exact identity", async () => {
    const { rules, calls } = fixture((call) => (call.target.scope === "project" ? done(["Keep tests green"], 2) : done(["Ask before pushing"], 5)));
    rules.setContext({ computer: "studio", project: "p-1", session });
    await tick();
    expect(calls[0]).toEqual({ computer: "studio", target: { scope: "project", project: "p-1" }, op: { op: "read" } });
    expect(rules.rules()).toEqual(["Keep tests green"]);
    expect(rules.scopes()).toBe(true);
    rules.setScope("session");
    await tick();
    expect(calls[1].target).toEqual({ scope: "session", ...session });
    expect(rules.rules()).toEqual(["Ask before pushing"]);
    expect(rules.revision()).toBe(5);
    // The same context reads nothing again; no session falls back to the project.
    rules.setContext({ computer: "studio", project: "p-1", session: { ...session } });
    expect(calls).toHaveLength(2);
    rules.setContext({ computer: "", project: "p-1" });
    expect(rules.scope()).toBe("project");
    expect(rules.scopes()).toBe(false);
  });

  it("adds and removes with the whole list at the revision it was read at; removing asks first", async () => {
    let stored = ["One", "Two"], revision = 1;
    const { rules, calls, asked } = fixture((call) => {
      if (call.op.op === "save") { stored = call.op.rules; revision++; }
      return done(stored, revision);
    });
    rules.setContext({ computer: "", project: "p" });
    await tick();
    expect(rules.canAdd("  ")).toBe(false);
    expect(await rules.add("Three")).toBe(true);
    expect(calls[1].op).toEqual({ op: "save", rules: ["One", "Two", "Three"], expected_revision: 1 });
    expect(await rules.remove(0)).toBe(true);
    expect(asked).toEqual([["One", 1]]);
    expect(calls[2].op).toEqual({ op: "save", rules: ["Two", "Three"], expected_revision: 2 });
    expect(rules.rules()).toEqual(["Two", "Three"]);
  });

  it("Cancel keeps the rule; a full list takes no more", async () => {
    const { rules, calls } = fixture(() => done(Array.from({ length: MAX_RULES }, (_, i) => `Rule ${i + 1}`), 1), false);
    rules.setContext({ computer: "", project: "p" });
    await tick();
    expect(await rules.remove(3)).toBe(false);
    expect(rules.canAdd("One more")).toBe(false);
    expect(await rules.add("One more")).toBe(false);
    expect(calls).toHaveLength(1);
  });

  it("a refused write keeps the rules; one that may have run reads them again; an old hgs stops editing", async () => {
    let answer: RulesOutcome = done(["One"], 1);
    const { rules, calls } = fixture(() => answer);
    rules.setContext({ computer: "", project: "p" });
    await tick();
    answer = { outcome: "failed", detail: { source: "%1", args: [{ kind: "verbatim", value: "Project rules changed; read them again" }] } };
    expect(await rules.add("Two")).toBe(false);
    expect(rules.rules()).toEqual(["One"]);
    expect(rules.error()?.args?.[0]).toEqual({ kind: "verbatim", value: "Project rules changed; read them again" });
    expect(rules.editable()).toBe(true);
    answer = { outcome: "uncertain", detail: { source: "%1", args: [{ kind: "verbatim", value: "timed out" }] }, request_id: "11111111-1111-4111-8111-111111111111" };
    const saving = rules.add("Two");
    answer = done(["One", "Two"], 2);
    expect(await saving).toBe(false);
    await tick();
    expect(calls.at(-1)?.op).toEqual({ op: "read" });
    expect(rules.rules()).toEqual(["One", "Two"]);
    expect(rules.error()?.args?.[0]).toEqual({ kind: "verbatim", value: "timed out" });
    answer = { outcome: "unsupported", detail: { source: "Update this computer's hgs to edit iron rules. Existing rules are kept." } };
    await rules.reload();
    expect(rules.failed()).toBe(true);
    expect(rules.editable()).toBe(false);
  });

  it("shows nothing without a project or a session", () => {
    const { rules, calls } = fixture(() => done([], 0));
    rules.setContext({ computer: "", project: "" });
    expect(rules.visible()).toBe(false);
    expect(calls).toHaveLength(0);
  });
});
