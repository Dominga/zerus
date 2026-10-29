import { describe, expect, it } from "vitest";
import { DEFAULT_RULES, controls, createWorktrees, type Rules, type View, type WorktreesState } from "./worktrees-page";

/** A promise the test settles. */
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
const settle = () => new Promise((done) => setTimeout(done, 0));
const RULES: Rules = { enabled: true, after_session: true, unused_integrated: false, missing: true, grace_hours: 24 };
const view = (change: Partial<View> = {}): View => ({
  peers: ["mac"], route: "", busy: false, rules: RULES, loaded: 1, status: "Cleanup settings loaded. Uncertain removals stay for manual review.",
  failed: false, version: 1, ...change,
});

function harness() {
  const calls: { name: string; arg?: unknown; reply: ReturnType<typeof deferred<View>> }[] = [];
  const call = (name: string, arg?: unknown) => { const reply = deferred<View>(); calls.push({ name, arg, reply }); return reply.promise; };
  let shown: WorktreesState | undefined;
  const page = createWorktrees({
    view: () => call("view"),
    read: () => call("read"),
    choose: (route) => call("choose", route),
    apply: (rules) => call("apply", rules),
  }, (state) => { shown = state; });
  return { page, calls, shown: () => shown! };
}

describe("Worktrees controls", () => {
  it("follow WorktreeSettings::controls()", () => {
    const off = { computer: false, refresh: false, apply: false, enabled: false, others: false, grace: false };
    expect(controls(undefined, RULES)).toEqual(off);
    // A request runs: nothing is usable, not even the computer list.
    expect(controls(view({ busy: true }), RULES)).toEqual(off);
    expect(controls(view(), RULES, true)).toEqual(off);
    // A failed or missing read: only the computer and Refresh.
    expect(controls(view({ rules: null, failed: true }), RULES)).toEqual({ ...off, computer: true, refresh: true });
    // Read: the rules depend on the switch above them.
    expect(controls(view(), RULES)).toEqual({ computer: true, refresh: true, apply: true, enabled: true, others: true, grace: false });
    expect(controls(view(), { ...RULES, unused_integrated: true }).grace).toBe(true);
    expect(controls(view(), { ...RULES, enabled: false })).toEqual({ computer: true, refresh: true, apply: true, enabled: true, others: false, grace: false });
  });
});

describe("the Worktrees page", () => {
  it("takes each answer's rules and keeps edits until the next one", async () => {
    const h = harness();
    expect(h.page.state().rules).toEqual(DEFAULT_RULES);
    h.page.open();
    h.calls[0].reply.resolve(view());
    await settle();
    expect(h.shown().rules).toEqual(RULES);
    h.page.edit({ grace_hours: 72 });
    // The same answer again (an event) keeps the edit.
    h.page.changed();
    h.calls[1].reply.resolve(view());
    await settle();
    expect(h.shown().rules.grace_hours).toBe(72);
    // A new answer resets the switches to it.
    h.page.changed();
    h.calls[2].reply.resolve(view({ loaded: 2, version: 2, rules: { ...RULES, enabled: false } }));
    await settle();
    expect(h.shown().rules).toEqual({ ...RULES, enabled: false });
  });

  it("sends one apply for a double click", async () => {
    const h = harness();
    h.page.open();
    h.calls[0].reply.resolve(view());
    await settle();
    h.page.edit({ enabled: false });
    h.page.apply();
    h.page.apply();
    expect(h.calls.filter((c) => c.name === "apply")).toHaveLength(1);
    expect(h.calls[1].arg).toEqual({ ...RULES, enabled: false });
    h.calls[1].reply.resolve(view({ busy: true, version: 2, status: "Reading cleanup settings…" }));
    await settle();
    h.page.apply();
    h.page.refresh();
    h.page.choose("mac");
    expect(h.calls).toHaveLength(2);
  });

  it("never lets an older reply replace a newer view", async () => {
    const h = harness();
    h.page.open();
    h.calls[0].reply.resolve(view());
    await settle();
    h.page.apply();
    // The answer's event is quicker than the apply's own reply.
    h.page.changed();
    h.calls[2].reply.resolve(view({ loaded: 2, version: 3, status: "Cleanup settings saved on the selected computer." }));
    await settle();
    h.calls[1].reply.resolve(view({ busy: true, version: 2, status: "Reading cleanup settings…" }));
    await settle();
    const state = h.shown();
    expect(state.view?.busy).toBe(false);
    expect(state.view?.status).toBe("Cleanup settings saved on the selected computer.");
    expect(state.sending).toBe(false);
  });

  it("opened again during an apply shows it running, then its outcome", async () => {
    // The app refuses the read while the change runs and says so.
    const h = harness();
    h.page.open();
    expect(h.calls[0].name).toBe("read");
    h.calls[0].reply.resolve(view({ route: "mac", busy: true, version: 7, loaded: 3, status: "Reading cleanup settings…" }));
    await settle();
    expect(h.shown().view?.route).toBe("mac");
    expect(controls(h.shown().view, h.shown().rules).apply).toBe(false);
    h.page.changed();
    h.calls[1].reply.resolve(view({ route: "mac", version: 8, loaded: 4, status: "Cleanup settings saved on the selected computer." }));
    await settle();
    expect(h.shown().view?.status).toBe("Cleanup settings saved on the selected computer.");
    expect(controls(h.shown().view, h.shown().rules).apply).toBe(true);
  });

  it("asks the app to choose a computer, which forgets the policy first", async () => {
    const h = harness();
    h.page.open();
    h.calls[0].reply.resolve(view());
    await settle();
    h.page.choose("");
    expect(h.calls).toHaveLength(1);
    h.page.choose("mac");
    expect(h.calls[1]).toMatchObject({ name: "choose", arg: "mac" });
    h.calls[1].reply.resolve(view({ route: "mac", busy: true, rules: null, version: 2, status: "Reading cleanup settings…" }));
    await settle();
    expect(controls(h.shown().view, h.shown().rules)).toMatchObject({ apply: false, enabled: false, computer: false });
  });

  it("shows a failed read with nothing to apply, and a call that failed outright", async () => {
    const h = harness();
    h.page.open();
    h.calls[0].reply.resolve(view({ rules: null, failed: true, status: "Could not read cleanup settings." }));
    await settle();
    expect(controls(h.shown().view, h.shown().rules)).toMatchObject({ apply: false, enabled: false, others: false, grace: false, refresh: true });
    h.page.apply();
    expect(h.calls).toHaveLength(1);
    h.page.refresh();
    h.calls[1].reply.reject("command worktree_policy_read not found");
    await settle();
    expect(h.shown().error).toBe("command worktree_policy_read not found");
    expect(h.shown().sending).toBe(false);
  });

  it("follows a peer that left: back on this computer with nothing to apply", async () => {
    const h = harness();
    h.page.open();
    h.calls[0].reply.resolve(view({ route: "mac" }));
    await settle();
    h.page.changed();
    h.calls[1].reply.resolve(view({ peers: [], route: "", rules: null, version: 2 }));
    await settle();
    expect(h.shown().view?.route).toBe("");
    expect(controls(h.shown().view, h.shown().rules).apply).toBe(false);
  });
});
