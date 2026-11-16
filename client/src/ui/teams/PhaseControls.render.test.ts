// @vitest-environment happy-dom
// Normal data comes from real product writers through Core/model/coalescer.
// Explicit mutations below test stale, missing and partial window reads.
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { createComponent, createSignal } from "solid-js";
import { render } from "solid-js/web";
import source from "./fixtures/phase-controls.json";
import { PhaseControls } from "./PhaseControls";
import type { TeamCall, TeamOutcome, TeamView } from "../../store/teams";

beforeEach(() => {
  HTMLDialogElement.prototype.showModal = function () { this.open = true; };
  HTMLDialogElement.prototype.close = function () { this.open = false; };
});
let dispose: (() => void) | undefined;
afterEach(() => { dispose?.(); document.body.replaceChildren(); });
const stage = (name: keyof typeof source) => structuredClone(source[name].team) as unknown as TeamView;
const button = (node: HTMLElement, selector: string) => { const b = node.querySelector<HTMLButtonElement>(selector); expect(b, selector).not.toBeNull(); return b!; };
function fixture(name: keyof typeof source = "draft", mutate?: (v: TeamView) => void) {
  const initial = stage(name); mutate?.(initial);
  const [view, setView] = createSignal(initial), [failed, setFailed] = createSignal(false);
  const run = vi.fn<(call: TeamCall) => Promise<TeamOutcome>>().mockResolvedValue({ outcome: "done", receipt: {}, request_id: "fixture-request" });
  const changed = vi.fn(), node = document.createElement("div"); document.body.append(node);
  dispose = render(() => createComponent(PhaseControls, { get draft() { return view().plan!.draft!; }, get view() { return view(); }, busy: false, get readFailed() { return failed(); }, run, onChanged: changed }), node);
  return { node, initial, run, changed, setView, setFailed };
}
function reason(node: HTMLElement, text: string) {
  const field = node.querySelector<HTMLTextAreaElement>("textarea")!; field.value = text; field.dispatchEvent(new Event("input", { bubbles: true }));
}

it("sends the writer's exact stable relative scope rather than an ordinal phase number", async () => {
  const f = fixture(); const c = f.initial.plan!.draft!.controls!;
  expect(c.earlier.target!.phase).not.toBe(c.scope.phase - 1);
  button(f.node, "[data-phase-move-earlier]").click();
  await vi.waitFor(() => expect(f.run).toHaveBeenCalledTimes(1));
  expect(f.run.mock.calls[0][0]).toMatchObject({ team: f.initial.bar.id, expected_revision: f.initial.plan!.draft!.revision,
    action: { kind: "move_phase", scope: c.scope, relative_to: c.earlier.target, placement: "before" } });
});

it("requires the bounded pause reason and sends the exact scope with explicit absence", async () => {
  const f = fixture(); button(f.node, "[data-phase-pause]").click();
  const apply = button(f.node, "[data-phase-control-submit]"); expect(apply.disabled).toBe(true);
  reason(f.node, "Ж".repeat(1025)); expect(apply.disabled).toBe(true);
  reason(f.node, "Review the retained phase gate."); apply.click();
  await vi.waitFor(() => expect(f.run).toHaveBeenCalledTimes(1));
  expect(f.run.mock.calls[0][0]).toMatchObject({ expected_revision: f.initial.plan!.draft!.revision,
    action: { kind: "pause_phase", scope: f.initial.plan!.draft!.controls!.scope, pause: null, reason: "Review the retained phase gate." } });
  expect(f.changed).toHaveBeenCalledTimes(1);
});

it("shows person pause attribution and resumes only its returned exact episode", async () => {
  const f = fixture("paused_person"); expect(f.node.textContent).toContain("Paused by the person");
  const p = f.initial.plan!.draft!.controls!.pause; expect(p.state).toBe("paused");
  if (p.state !== "paused") throw new Error("fixture is not paused");
  expect(f.node.textContent).toContain(p.episode!.reason);
  button(f.node, "[data-phase-resume]").click(); await vi.waitFor(() => expect(f.run).toHaveBeenCalledTimes(1));
  expect(f.run.mock.calls[0][0].action).toEqual({ kind: "resume_phase", scope: f.initial.plan!.draft!.controls!.scope, pause: p.episode!.id });
});

it("shows a lead pause and binds replacement to that current pause episode", async () => {
  const f = fixture("paused_lead"); expect(f.node.textContent).toContain("Paused by lead lead");
  const p = f.initial.plan!.draft!.controls!.pause; if (p.state !== "paused") throw new Error("fixture is not paused");
  button(f.node, "[data-phase-pause]").click(); reason(f.node, "Replace this reason explicitly."); button(f.node, "[data-phase-control-submit]").click();
  await vi.waitFor(() => expect(f.run).toHaveBeenCalledTimes(1));
  expect(f.run.mock.calls[0][0].action).toMatchObject({ kind: "pause_phase", pause: p.episode!.id });
});

it("locks an unknown original and never constructs a second request from a later read", async () => {
  const f = fixture(); f.run.mockResolvedValue({ outcome: "uncertain", request_id: "fixture-request", detail: "Lost original answer" });
  button(f.node, "[data-phase-pause]").click(); reason(f.node, "Keep the exact original."); button(f.node, "[data-phase-control-submit]").click();
  await vi.waitFor(() => expect(f.node.textContent).toContain("use Check"));
  button(f.node, "[data-phase-control-submit]").click(); expect(f.run).toHaveBeenCalledTimes(1);
  const newer = stage("paused_person"); f.setView(newer);
  button(f.node, "[data-phase-control-submit]").click(); expect(f.run).toHaveBeenCalledTimes(1);
});

it.each(["failed", "revision", "missing", "partial", "live-missing", "live-partial", "unknown"])("disables controls for a %s read", (fault) => {
  const f = fixture("approved_unstarted", (v) => {
    if (fault === "revision") v.bar.revision += 1;
    if (fault === "missing") delete v.plan!.draft!.controls;
    if (fault === "partial") v.plan!.draft!.complete = false;
    if (fault === "live-missing") delete v.plan!.draft!.live;
    if (fault === "live-partial") v.plan!.draft!.live!.complete = false;
    if (fault === "unknown") v.plan!.draft!.unknown_fields = true;
  });
  if (fault === "failed") f.setFailed(true);
  for (const b of f.node.querySelectorAll<HTMLButtonElement>("button")) expect(b.disabled).toBe(true);
  expect(f.run).not.toHaveBeenCalled();
});

it("keeps a draft form unavailable when its read changes without replacing its saved scope", () => {
  const f = fixture(); button(f.node, "[data-phase-pause]").click(); reason(f.node, "Keep this text to copy.");
  const newer = stage("draft"); newer.bar.revision++; newer.plan!.draft!.revision++; f.setView(newer);
  const apply = button(f.node, "[data-phase-control-submit]"); expect(apply.disabled).toBe(true); apply.click();
  expect(f.run).not.toHaveBeenCalled(); expect(f.node.textContent).toContain("read changed");
  expect(f.node.querySelector<HTMLTextAreaElement>("textarea")!.value).toBe("Keep this text to copy.");
});

it("offers an explicit acceptance choice only for a positively unstarted phase", async () => {
  const f = fixture("approved_unstarted"); button(f.node, "[data-phase-acceptance]").click();
  const select = f.node.querySelector<HTMLSelectElement>("[data-phase-acceptance-choice]")!;
  const value = f.initial.plan!.draft!.phase.acceptance === "person" ? "lead" : "person";
  select.value = value; select.dispatchEvent(new Event("change", { bubbles: true })); button(f.node, "[data-phase-control-submit]").click();
  await vi.waitFor(() => expect(f.run).toHaveBeenCalledTimes(1));
  expect(f.run.mock.calls[0][0].action).toEqual({ kind: "set_phase_acceptance", scope: f.initial.plan!.draft!.controls!.scope, acceptance: value });
  f.setView(stage("awaiting")); expect(button(f.node, "[data-phase-acceptance]").disabled).toBe(true);
});

it("retains exact BASE and NEW identities while only the reserved correction makes live scope incomplete", async () => {
  const f = fixture("correction_publishing"), d = f.initial.plan!.draft!, c = d.controls!;
  expect(d.live!.complete).toBe(false); expect(c.scope.kind).toBe("published");
  if (c.scope.kind !== "published") throw new Error("fixture is not published");
  expect(c.scope.rework!.approval).not.toBe(c.scope.publication);
  expect(button(f.node, "[data-phase-resume]").disabled).toBe(false);
  expect(button(f.node, "[data-phase-acceptance]").disabled).toBe(true);
  button(f.node, "[data-phase-resume]").click(); await vi.waitFor(() => expect(f.run).toHaveBeenCalledTimes(1));
  expect(f.run.mock.calls[0][0].action.scope).toEqual(c.scope);
});


it.each(["stale_revision", "uncertain", "throw"])("keeps surrounding controls locked after closing a %s pause form", async (response) => {
  const f = fixture();
  if (response === "throw") f.run.mockRejectedValue(new Error("Lost original answer"));
  else f.run.mockResolvedValue(response === "uncertain"
    ? { outcome: "uncertain", request_id: "fixture-request", detail: "Lost original answer" }
    : { outcome: "failed", request_id: "fixture-request", code: "stale_revision", detail: "Read again before a new request." });
  button(f.node, "[data-phase-pause]").click(); reason(f.node, "Keep this original reason."); button(f.node, "[data-phase-control-submit]").click();
  await vi.waitFor(() => expect(f.node.textContent).toContain(response === "stale_revision" ? "Read again before a new request." : "use Check"));
  expect(button(f.node, "[data-phase-control-submit]").disabled).toBe(true);
  const close = Array.from(f.node.querySelectorAll<HTMLButtonElement>("dialog button")).find((b) => b.textContent === "Close")!;
  expect(() => close.click()).not.toThrow();
  await vi.waitFor(() => expect(f.node.querySelector("dialog")).toBeNull());
  for (const control of f.node.querySelectorAll<HTMLButtonElement>("button")) expect(control.disabled).toBe(true);
  button(f.node, "[data-phase-barrier]").click(); button(f.node, "[data-phase-pause]").click();
  expect(f.run).toHaveBeenCalledTimes(1);
});

it("keeps direct controls locked after a stale original instead of creating another request", async () => {
  const f = fixture(); f.run.mockResolvedValue({ outcome: "failed", request_id: "fixture-request", code: "stale_revision", detail: "Read again before a new request." });
  button(f.node, "[data-phase-barrier]").click();
  await vi.waitFor(() => expect(f.node.textContent).toContain("Read again before a new request."));
  for (const control of f.node.querySelectorAll<HTMLButtonElement>("button")) expect(control.disabled).toBe(true);
  button(f.node, "[data-phase-barrier]").click(); expect(f.run).toHaveBeenCalledTimes(1);
});
