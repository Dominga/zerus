import { describe, expect, it } from "vitest";
import { TeamActions, allDecided, effectivePhaseMode, freeMode, phaseModeChip, phaseModeConfirmation, phaseModeState, phaseModeSwitch, teamModeBadge, type ActTeam } from "./actions";
import { proposalText } from "./decisions";
import type { DecisionPort } from "./decisions";
import type { TeamCall, TeamConfirmation, TeamProposalView } from "../../types";

// Task #58: the owner switches one phase between free and standard planning
// (Qt's teamPhaseModeDialog), confirmed first; the lead's card names its phase.

function team(over: Partial<ActTeam> = {}): ActTeam {
  return {
    id: "t1", name: "Checkout", status: "running", plan_mode: "standard", revision: 7, pending_phase: null, waiting_for_lead: false, settings: {},
    members: [{ id: "lead", role: "Lead", agent: "claude", computer: "", lead: true, session: { name: "claude/x/lead" } }],
    tasks: [{ id: 1, title: "API", status: "accepted", assignee: "lead", phase: 1, deps: [], attempts: 1 }],
    phases: [
      { n: 1, title: "Build", state: "done", status: "done", mode: "standard" },
      { n: 2, title: "Support", state: "running", status: "in_work", mode: "standard" },
      { n: 3, title: "Polish", state: "draft", status: "draft", mode: "standard" },
    ],
    free_phase: null, phase_modes_known: true,
    ...over,
  };
}

function fake(yes: boolean): { port: DecisionPort; sent: TeamCall[]; asked: TeamConfirmation[] } {
  const f = {
    sent: [] as TeamCall[], asked: [] as TeamConfirmation[],
    port: {
      async send(call: TeamCall) { f.sent.push(call); return { outcome: "done", receipt: { status: "switched" }, request_id: "r1" }; },
      async confirm(ask: TeamConfirmation) { f.asked.push(ask); return yes; },
      async choose() { return null; },
    } as unknown as DecisionPort,
  };
  return f;
}

describe("a phase's mode", () => {
  it("a running phase opens to free, the free phase locks back, a draft offers nothing", () => {
    const t = team();
    expect(phaseModeSwitch(t, 2)).toMatchObject({ mode: "free", enabled: true, text: "Switch to free planning…" });
    expect(phaseModeSwitch(t, 1)).toBeUndefined();
    expect(phaseModeSwitch(t, 3)).toBeUndefined();
    const free = team({ free_phase: 2, phases: t.phases.map((phase) => (phase.n === 2 ? { ...phase, mode: "free" } : phase)) });
    expect(phaseModeSwitch(free, 2)).toMatchObject({ mode: "standard", enabled: true, text: "Switch to standard…" });
    // One phase is free at a time.
    const other = team({ free_phase: 4, phases: [...t.phases, { n: 4, title: "Stream", state: "running", status: "in_work", mode: "free" }] });
    expect(phaseModeSwitch(other, 2)).toMatchObject({ mode: "free", enabled: false, why: "Phase 4 is free; switch it to standard first" });
    // An older hgs or a team that is not running offers no switch.
    expect(phaseModeSwitch(team({ phase_modes_known: false }), 2)).toBeUndefined();
    expect(phaseModeSwitch(team({ status: "planning" }), 2)).toBeUndefined();
  });

  it("every phase header's toggle names its mode and what the owner may do", () => {
    const t = team();
    expect(phaseModeState(t, 2, true)).toMatchObject({ mode: "standard", target: "free", canSwitch: true, draft: false, readOnly: false });
    expect(phaseModeState(t, 2, false)).toMatchObject({ mode: "standard", canSwitch: false, readOnly: true,
      why: "Only the owner switches a phase's mode; the lead proposes it" });
    const free = team({ free_phase: 2, phases: t.phases.map((phase) => (phase.n === 2 ? { ...phase, mode: "free" } : phase)) });
    expect(phaseModeState(free, 2, true)).toMatchObject({ mode: "free", target: "standard", canSwitch: true });
    // A draft shows the mode it will start in and says why it cannot switch yet.
    expect(phaseModeState(t, 3, true)).toMatchObject({ mode: "standard", draft: true, canSwitch: false });
    expect(phaseModeState(t, 3, true)?.why).toContain("draft phase starts in Standard");
  });

  it("a pending switch-plan-mode proposal is marked on the toggle", () => {
    const t = team({ proposals: [{ kind: "switch-plan-mode", mode: "free", change: { phase: 2 } }] });
    expect(phaseModeState(t, 2, true)).toMatchObject({ mode: "standard", proposed: "free", target: "free" });
    expect(phaseModeState(t, 1, true)?.proposed).toBeUndefined();
    expect(phaseModeState(t, 2, true)?.tip).toContain("Free");
  });

  it("a free phase keeps the queue idle, whichever phase it is", () => {
    expect(allDecided(team())).toBe(true);
    expect(freeMode(team({ free_phase: 2 }))).toBe(true);
    expect(allDecided(team({ free_phase: 2 }))).toBe(false);
    // A free team whose phase was locked has no free phase.
    expect(freeMode(team({ plan_mode: "free", free_phase: null }))).toBe(false);
    // An older hgs: the team's mode.
    expect(freeMode(team({ plan_mode: "free", phase_modes_known: false }))).toBe(true);
  });

  it("asks Qt's question first, then sends phase_mode once; Cancel sends nothing", async () => {
    const t = team();
    const ask = phaseModeConfirmation(t, 2, "free");
    expect(ask).toMatchObject({ name: "teamPhaseModeConfirm", title: "Switch Phase 2: Support to free planning?", action: "Switch to free" });
    expect(ask.text).toContain("The other phases keep their modes.");
    expect(phaseModeConfirmation(t, 2, "standard").text).toContain("its tasks keep their state and run on");
    const no = fake(false);
    expect(await new TeamActions(no.port, () => ({ computer: "", team: t })).phaseMode(2, "free")).toBe("cancelled");
    expect(no.sent).toEqual([]);
    const yes = fake(true);
    await new TeamActions(yes.port, () => ({ computer: "", team: t })).phaseMode(2, "standard");
    expect(yes.asked.map((asked) => asked.name)).toEqual(["teamPhaseModeConfirm"]);
    expect(yes.sent).toEqual([{ computer: "", team: "t1", team_name: "Checkout", confirmed: true, request_id: null, action: { kind: "phase_mode", phase: 2, mode: "standard" } }]);
  });

  it("task #12: each phase reads its effective mode, its own else the team's, in every state", () => {
    const own = (mode: string) => (phase: ActTeam["phases"][number]) => (phase.n === 3 ? { ...phase, mode } : { ...phase, mode: "" });
    const freeTeam = team({ plan_mode: "free", phases: team().phases.map(own("")) });
    expect([1, 2, 3].map((n) => effectivePhaseMode(freeTeam, n))).toEqual(["free", "free", "free"]);
    const withOwn = team({ plan_mode: "free", phases: team().phases.map(own("standard")) });
    expect([1, 2, 3].map((n) => effectivePhaseMode(withOwn, n))).toEqual(["free", "free", "standard"]);
    expect(phaseModeState(withOwn, 3, true)?.mode).toBe("standard");
    expect(effectivePhaseMode(team({ free_phase: 2 }), 2)).toBe("free");
  });

  it("task #12: while the team plans, the team switch and each phase toggle are stored at once", async () => {
    const plans = team({ status: "planning", phases: team().phases.map((phase) => ({ ...phase, state: "draft", status: "draft" })) });
    expect(phaseModeState(plans, 2, true)).toMatchObject({ canSwitch: true, why: "" });
    expect(phaseModeState(plans, 2, false)?.canSwitch).toBe(false);
    const f = fake(false);
    await new TeamActions(f.port, () => ({ computer: "", team: plans })).phaseMode(2, "free");
    await new TeamActions(f.port, () => ({ computer: "", team: plans })).teamMode("free");
    expect(f.asked).toEqual([]);
    expect(f.sent.map((call) => call.action)).toEqual([{ kind: "phase_mode", phase: 2, mode: "free" }, { kind: "team_mode", mode: "free" }]);
    expect(f.sent.every((call) => call.confirmed === true)).toBe(true);
    // A running team's switch asks first; Cancel sends nothing.
    const running = fake(false);
    expect(await new TeamActions(running.port, () => ({ computer: "", team: team() })).teamMode("free")).toBe("cancelled");
    expect(running.asked.map((ask) => ask.name)).toEqual(["teamModeConfirm"]);
    expect(running.asked[0].text).toContain("a phase with its own mode keeps it");
    expect(running.sent).toEqual([]);
  });

  it("the lead's card names the phase it switches", () => {
    const card = (change: unknown) => ({ id: "p7", kind: "switch-plan-mode", by: "Lead", at: 0, reason: "", hash: "h", summary: "", lines: [], change, member: null, mode: "standard" }) as unknown as TeamProposalView;
    expect(proposalText(team() as never, card({ phase: 2 }))).toBe("Switch phase 2 to standard planning");
    expect(proposalText(team() as never, card(null))).toBe("Switch to standard planning");
  });
});

// Task #59: the phase header and the team bar show the mode hgs reports, live:
// each view of the team is drawn from the latest status read.
describe("the mode badges", () => {
  it("a running phase reads Standard, the free phase Free, a draft or done phase nothing", () => {
    expect(phaseModeChip(team(), 2)).toMatchObject({ mode: "standard", text: "Standard mode" });
    expect(phaseModeChip(team(), 1)).toBeUndefined();
    expect(phaseModeChip(team(), 3)).toBeUndefined();
    const free = team({ free_phase: 2, phases: team().phases.map((p) => (p.n === 2 ? { ...p, mode: "free" } : p)) });
    expect(phaseModeChip(free, 2)).toMatchObject({ mode: "free", text: "Free mode" });
    // An older hgs without phase modes: no Standard chip to guess.
    expect(phaseModeChip(team({ phase_modes_known: false }), 2)).toBeUndefined();
  });

  it("the team bar names the free phase and follows the next read", () => {
    expect(teamModeBadge(team())).toMatchObject({ mode: "standard", text: "Standard mode", phase: 0 });
    const free = team({ free_phase: 2 });
    expect(teamModeBadge(free)).toMatchObject({ mode: "free", text: "Free mode: phase 2", phase: 2 });
    expect(phaseModeSwitch(free, 2)).toMatchObject({ mode: "standard", text: "Switch to standard…" });
    expect(teamModeBadge(team({ status: "planning" }))).toBeUndefined();
    expect(teamModeBadge(team({ phase_modes_known: false, plan_mode: "free" }))).toMatchObject({ mode: "free", text: "Free mode" });
  });
});
