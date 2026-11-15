// @vitest-environment happy-dom
// Normal states are the real CLI/task/archive writers through the core reader,
// model and batch. Explicit mutations below isolate stale and unavailable UI.
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { createComponent, createSignal } from "solid-js";
import { render } from "solid-js/web";
import source from "./fixtures/phase-publication.json";
import { PhaseActions } from "./PhaseActions";
import { PhaseLiveLayers } from "./PhaseLiveLayers";
import { phaseChanges } from "./PhaseReview";
import { WaitingCard } from "./Waiting";
import { phaseFocus, takePhaseFocus } from "../../store/team-phases";
import { teamPlanPreferences } from "../../store/ui";
import { openTask } from "../../store/taskNavigation";
import type { TeamCall, TeamOutcome, TeamView, WaitingItem } from "../../store/teams";

vi.mock("../../store/taskNavigation", async (original) => ({ ...await original<typeof import("../../store/taskNavigation")>(), openTask: vi.fn() }));
const disposers: (() => void)[] = [];
const stage = (name: keyof typeof source) => structuredClone(source[name].team) as unknown as TeamView;
const team = () => stage("revised").bar.id;
beforeEach(() => teamPlanPreferences.update(team(), { filter: "all", showCancelled: false, hideFinished: false, allCollapsed: false, sections: [] }));
afterEach(() => { disposers.splice(0).forEach((d) => d()); document.body.replaceChildren(); takePhaseFocus(team()); vi.clearAllMocks(); });
function host() { const node = document.createElement("div"); document.body.append(node); return node; }
function button(node: HTMLElement, selector: string) { return node.querySelector<HTMLButtonElement>(selector)!; }
function fixture(name: keyof typeof source = "revised", over?: (v: TeamView) => void) {
  const initial = stage(name); over?.(initial);
  const [view, setView] = createSignal(initial);
  const run = vi.fn<(call: TeamCall) => Promise<TeamOutcome>>().mockResolvedValue({ outcome: "done", receipt: {}, request_id: "fixture-request" });
  const changed = vi.fn(), node = host();
  disposers.push(render(() => createComponent(PhaseActions, { get draft() { return view().plan!.draft!; }, get view() { return view(); }, busy: false, available: true, readFailed: false, run, onChanged: changed }), node));
  return { node, run, changed, initial, setView };
}

it("reviews the original generation, roster and folders and keeps its revision when a newer read arrives", async () => {
  const f = fixture(); button(f.node, "[data-phase-approve]").click();
  expect(f.node.textContent).toContain("This initial approval also confirms the roster");
  expect(f.node.textContent).toContain("folder is not created yet");
  expect(f.node.textContent).toContain("example-planning-account");
  expect(f.node.textContent).toContain("The regression is covered through the real writer");
  const newer = stage("revised"); newer.bar.revision += 100; newer.plan!.draft!.revision += 100; newer.plan!.draft!.generation = 3; f.setView(newer);
  button(f.node, "[data-phase-review-submit]").click();
  await vi.waitFor(() => expect(f.run).toHaveBeenCalledTimes(1));
  expect(f.run.mock.calls[0][0]).toMatchObject({ team: f.initial.bar.id, expected_revision: f.initial.plan!.draft!.revision,
    action: { kind: "approve_phase", phase: 1, generation: 2, confirm_initial: true } });
  expect(f.changed).toHaveBeenCalledTimes(1);
});

it("requires a rejection comment and sends it unchanged", async () => {
  const f = fixture(); button(f.node, "[data-phase-reject]").click();
  const submit = button(f.node, "[data-phase-review-submit]"); expect(submit.disabled).toBe(true);
  const field = f.node.querySelector<HTMLTextAreaElement>("textarea")!;
  field.value = "Explain the failure case.\nKeep the existing API."; field.dispatchEvent(new Event("input", { bubbles: true }));
  submit.click(); await vi.waitFor(() => expect(f.run).toHaveBeenCalledTimes(1));
  expect(f.run.mock.calls[0][0].action).toMatchObject({ kind: "reject_phase", phase: 1, generation: 2, comment: field.value });
});

it("keeps an uncertain approval locked and offers the existing Check path", async () => {
  const f = fixture(); f.run.mockResolvedValue({ outcome: "uncertain", request_id: "fixture-request", detail: "Lost answer" });
  button(f.node, "[data-phase-approve]").click(); button(f.node, "[data-phase-review-submit]").click();
  await vi.waitFor(() => expect(f.node.textContent).toContain("use Check"));
  button(f.node, "[data-phase-review-submit]").click(); expect(f.run).toHaveBeenCalledTimes(1);
});

it("does not combine a phase with a roster from another revision", () => {
  const f = fixture("revised", (v) => { v.bar.revision += 1; });
  button(f.node, "[data-phase-approve]").click();
  expect(f.node.querySelector("dialog")).toBeNull(); expect(f.node.textContent).toContain("revisions differ"); expect(f.run).not.toHaveBeenCalled();
});

it("undoes only the displayed rejection episode", async () => {
  const f = fixture("rejected"); button(f.node, "[data-phase-undo]").click();
  await vi.waitFor(() => expect(f.run).toHaveBeenCalledTimes(1));
  const rejection = f.initial.plan!.draft!.rejection!;
  expect(f.run.mock.calls[0][0].action).toEqual({ kind: "undo_reject_phase", phase: 1, generation: rejection.generation, rejection: rejection.id });
});

it("shows partial task creation with no second approval or draft edit", () => {
  const f = fixture("partial"); expect(f.node.textContent).toContain("Task creation: 1 of 2 confirmed");
  expect(f.node.querySelector("[data-phase-approve]")).toBeNull(); expect(f.node.querySelector("[data-phase-reject]")).toBeNull();
});

it("accepts the actual completed publication and disables acceptance on an incomplete live scope", async () => {
  const f = fixture("awaiting"); button(f.node, "[data-phase-accept]").click();
  await vi.waitFor(() => expect(f.run).toHaveBeenCalledTimes(1));
  expect(f.run.mock.calls[0][0].action).toEqual({ kind: "accept_phase", phase: 1, publication: f.initial.plan!.draft!.publication!.id });
  const bad = stage("awaiting"); bad.plan!.draft!.live!.complete = false; f.setView(bad);
  expect(button(f.node, "[data-phase-accept]").disabled).toBe(true);
});

it("uses the retained producer generation for the draft diff", () => {
  const changes = phaseChanges(stage("revised").plan!.draft!)!;
  expect(changes.added).toHaveLength(0); expect(changes.removed).toHaveLength(0); expect(changes.changed.map((n) => n.id)).toEqual([1]);
});

it("opens the exact phase named by the service's owner card", () => {
  const item = structuredClone(source.draft.waiting.items[0]) as WaitingItem;
  const opened = vi.fn(), node = host();
  disposers.push(render(() => createComponent(WaitingCard, { item, teamName: "Fixture", now: 0, busy: false, onDecide: vi.fn(), onOpenTeam: opened }), node));
  button(node, "[data-open-phase]").click();
  expect(phaseFocus()).toMatchObject({ team: item.team, phase: item.phase_reference!.phase }); expect(opened).toHaveBeenCalledWith(item);
});

it("uses supplied live layers, shared toolbar filters and the ordinary task link", () => {
  const view = stage("published"), draft = view.plan!.draft!, live = draft.live!;
  draft.graph.nodes[0].layer = 7; // Deliberate projection control, not a new row.
  const node = host(); disposers.push(render(() => createComponent(PhaseLiveLayers, { team: view.bar.id, draft, live }), node));
  const article = node.querySelector<HTMLElement>('[aria-label="Layer 8"] [data-phase-node="1"]')!;
  expect(article).not.toBeNull(); button(article, ".phase-node-title").click();
  expect(article.textContent).toContain("The regression is covered through the real writer");
  button(article, ".btn").click(); expect(openTask).toHaveBeenCalledWith(live.nodes[0].task!.project, live.nodes[0].task!.uuid);
  button(node, '[data-plan-filter="active"]').click(); expect(node.querySelectorAll("[data-phase-node]")).toHaveLength(0);
  expect(node.textContent).toContain("filters hide every task"); expect(live.nodes).toHaveLength(2);
});
