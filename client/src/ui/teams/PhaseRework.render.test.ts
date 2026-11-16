// @vitest-environment happy-dom
// Normal contexts are real CLI/task/archive writers through core/model/batch.
// Explicit mutations isolate a changed read, missing capability and read failure.
import { afterEach, expect, it, vi } from "vitest";
import { createComponent, createSignal } from "solid-js";
import { render } from "solid-js/web";
import source from "./fixtures/phase-rework.json";
import { PhaseRework } from "./PhaseRework";
import type { TeamCall, TeamOutcome, TeamView } from "../../store/teams";

const disposers: (() => void)[] = [];
afterEach(() => { disposers.splice(0).forEach((d) => d()); document.body.replaceChildren(); });
const stage = (name: keyof typeof source) => structuredClone(source[name].team) as unknown as TeamView;
function fixture(name: keyof typeof source, fault?: (view: TeamView) => void, failed = false) {
  const initial = stage(name); fault?.(initial);
  const [view, setView] = createSignal(initial);
  const run = vi.fn<(call: TeamCall) => Promise<TeamOutcome>>().mockResolvedValue({ outcome: "done", receipt: {}, request_id: "fixture-request" });
  const changed = vi.fn(), node = document.createElement("div"); document.body.append(node);
  disposers.push(render(() => createComponent(PhaseRework, { get view() { return view(); }, get draft() { return view().plan!.draft!; },
    get routes() { return view().plan!.list!.routes; }, busy: false, readFailed: failed, run, onChanged: changed }), node));
  return { node, run, changed, initial, setView };
}
function button(node: HTMLElement, selector: string) { return node.querySelector<HTMLButtonElement>(selector)!; }
function input(node: HTMLElement, selector: string, text: string) {
  const field = node.querySelector<HTMLInputElement | HTMLTextAreaElement>(selector)!;
  field.value = text; field.dispatchEvent(new Event("input", { bubbles: true }));
}

it("sends a required comment for the original publication/revision even after a newer read", async () => {
  const f = fixture("awaiting"); button(f.node, "[data-phase-send-back]").click();
  expect(button(f.node, "[data-phase-return-submit]").disabled).toBe(true);
  const comment = "Keep accepted work.\nAdd the missing regression.";
  input(f.node, "[data-phase-return-comment]", comment);
  const newer = stage("awaiting"); newer.bar.revision += 100; newer.plan!.draft!.revision += 100;
  newer.plan!.draft!.publication!.id = "00000000-0000-4000-8000-000000000099"; f.setView(newer);
  button(f.node, "[data-phase-return-submit]").click();
  await vi.waitFor(() => expect(f.run).toHaveBeenCalledTimes(1));
  expect(f.run.mock.calls[0][0]).toMatchObject({ expected_revision: f.initial.plan!.draft!.revision,
    action: { kind: "send_back_phase", phase: 1, publication: f.initial.plan!.draft!.publication!.id, comment } });
  expect(f.changed).toHaveBeenCalledTimes(1);
});

it("locks an uncertain SendBack and retains its comment without a second request", async () => {
  const f = fixture("awaiting"); f.run.mockResolvedValue({ outcome: "uncertain", request_id: "fixture-request", detail: "Lost reply" });
  button(f.node, "[data-phase-send-back]").click(); input(f.node, "[data-phase-return-comment]", "Keep this correction reason.");
  button(f.node, "[data-phase-return-submit]").click();
  await vi.waitFor(() => expect(f.node.textContent).toContain("use Check"));
  expect(button(f.node, "[data-phase-return-submit]").disabled).toBe(true);
  button(f.node, "[data-phase-return-submit]").click(); expect(f.run).toHaveBeenCalledTimes(1);
  expect(f.node.querySelector<HTMLTextAreaElement>("textarea")!.value).toBe("Keep this correction reason.");
});

it("shows a separate correction proposal while the earlier phase still awaits acceptance", () => {
  const f = fixture("proposal"), root = f.initial.plan!.draft!, proposal = root.rework!.draft!;
  expect(root.phase.state).toBe("awaiting_acceptance"); expect(root.accept_available).toBe(false);
  expect(f.node.textContent).toContain("Previously accepted work stays unchanged");
  expect(f.node.textContent).toContain("still awaiting acceptance");
  expect([...f.node.querySelectorAll<HTMLElement>("[data-phase-node]")].map((n) => Number(n.dataset.phaseNode))).toEqual(proposal.nodes.map((n) => n.id));
  expect(f.node.querySelector("[data-phase-accept]")).toBeNull(); expect(f.run).not.toHaveBeenCalled();
});

it("freezes the exact base/rework/generation for a correction approval", async () => {
  const f = fixture("proposal"), root = f.initial.plan!.draft!;
  button(f.node, "[data-phase-approve]").click();
  expect(f.node.textContent).toContain("Approve corrections");
  expect(f.node.textContent).toContain("only the explicit correction tasks");
  const newer = stage("proposal"); newer.bar.revision += 100; newer.plan!.draft!.revision += 100;
  newer.plan!.draft!.rework!.id = "00000000-0000-4000-8000-000000000098";
  newer.plan!.draft!.rework!.draft!.generation = 100; f.setView(newer);
  button(f.node, "[data-phase-review-submit]").click(); await vi.waitFor(() => expect(f.run).toHaveBeenCalledTimes(1));
  expect(f.run.mock.calls[0][0]).toMatchObject({ expected_revision: root.revision, action: { kind: "approve_phase_corrections", phase: 1,
    publication: root.rework!.base_publication, rework: root.rework!.id, generation: root.rework!.draft!.generation } });
  expect(f.run.mock.calls[0][0].action).not.toHaveProperty("confirm_initial");
});

it("rejects only the displayed correction episode with the required comment", async () => {
  const f = fixture("proposal"), root = f.initial.plan!.draft!; button(f.node, "[data-phase-reject]").click();
  expect(button(f.node, "[data-phase-review-submit]").disabled).toBe(true);
  input(f.node, "textarea", "Make the failure case explicit."); button(f.node, "[data-phase-review-submit]").click();
  await vi.waitFor(() => expect(f.run).toHaveBeenCalledTimes(1));
  expect(f.run.mock.calls[0][0].action).toEqual({ kind: "reject_phase_corrections", phase: 1, publication: root.rework!.base_publication,
    rework: root.rework!.id, generation: root.rework!.draft!.generation, comment: "Make the failure case explicit." });
});

it("undoes the exact correction rejection, preserving the separate base publication", async () => {
  const f = fixture("rejected"), r = f.initial.plan!.draft!.rework!; button(f.node, "[data-phase-undo]").click();
  await vi.waitFor(() => expect(f.run).toHaveBeenCalledTimes(1));
  expect(f.run.mock.calls[0][0].action).toEqual({ kind: "undo_phase_correction_rejection", phase: 1, publication: r.base_publication,
    rework: r.id, generation: r.draft!.rejection!.generation, rejection: r.draft!.rejection!.id });
});

it("edits with the original episode and keeps the phase barrier fixed", async () => {
  const f = fixture("proposal"), root = f.initial.plan!.draft!; button(f.node, "[data-edit-corrections]").click();
  const dialog = f.node.querySelector("dialog")!;
  const barrier = dialog.querySelector<HTMLInputElement>('input[type="checkbox"]')!; expect(barrier.disabled).toBe(true);
  input(dialog, "[data-phase-title]", "A precise correction plan");
  const newer = stage("proposal"); newer.plan!.draft!.rework!.id = "00000000-0000-4000-8000-000000000098"; newer.bar.revision += 100; f.setView(newer);
  button(f.node, "[data-save-phase]").click(); await vi.waitFor(() => expect(f.run).toHaveBeenCalledTimes(1));
  expect(f.run.mock.calls[0][0]).toMatchObject({ expected_revision: root.revision, action: { kind: "edit_phase_corrections", phase: 1,
    publication: root.rework!.base_publication, rework: root.rework!.id, generation: root.rework!.draft!.generation,
    draft: { title: "A precise correction plan", barrier: !!root.phase.barrier } } });
});

it("never sends an old correction editor into a newly selected team", async () => {
  const f = fixture("proposal"); button(f.node, "[data-edit-corrections]").click(); input(f.node, "[data-phase-title]", "Keep my correction text");
  const other = stage("proposal"); other.bar.id = "another-team"; f.setView(other);
  button(f.node, "[data-save-phase]").click();
  await vi.waitFor(() => expect(button(f.node, "[data-save-phase]").disabled).toBe(true));
  expect(f.node.textContent).toContain("your text stays here to copy");
  expect(f.run).not.toHaveBeenCalled(); expect(f.node.querySelector<HTMLInputElement>("[data-phase-title]")!.value).toBe("Keep my correction text");
});

it("starts a nonempty explicit proposal with the read's next node, not accepted task IDs", async () => {
  const f = fixture("sent_back"), root = f.initial.plan!.draft!; button(f.node, "[data-edit-corrections]").click();
  input(f.node, "[data-node-title]", "Add the regression"); input(f.node, "[data-node-criteria]", "The missing case passes");
  button(f.node, "[data-save-phase]").click(); await vi.waitFor(() => expect(f.run).toHaveBeenCalledTimes(1));
  expect(f.run.mock.calls[0][0].action).toMatchObject({ kind: "submit_phase_corrections", phase: 1, publication: root.rework!.base_publication,
    rework: root.rework!.id, draft: { nodes: [{ id: root.next_node, title: "Add the regression" }] } });
  expect(root.nodes.some((n) => n.id === root.next_node)).toBe(false);
});

it("keeps a partial correction publication frozen with no second approval or edit", () => {
  const f = fixture("partial"); expect(button(f.node, "[data-edit-corrections]").disabled).toBe(true);
  expect(f.node.querySelector("[data-phase-approve],[data-phase-reject],[data-phase-send-back]")).toBeNull();
  expect(f.node.textContent).toContain("Task creation: 1 of 2 confirmed");
});

it("explains the missing capability and disables actions after a failed read", () => {
  const old = fixture("awaiting", (v) => { delete v.plan!.draft!.rework_available; delete v.plan!.draft!.rework; });
  expect(button(old.node, "[data-phase-send-back]").disabled).toBe(true); expect(old.node.textContent).toContain("not available on this computer");
  const failed = fixture("proposal", undefined, true); expect(button(failed.node, "[data-edit-corrections]").disabled).toBe(true);
  expect(button(failed.node, "[data-phase-approve]").disabled).toBe(true); expect(failed.run).not.toHaveBeenCalled();
});
