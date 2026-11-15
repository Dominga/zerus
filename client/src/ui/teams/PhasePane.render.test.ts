// @vitest-environment happy-dom
// The normal rows come from the CLI writer through the real core reader,
// model, graph projection and batch; the committed core test guards this file.
import { afterEach, expect, it, vi } from "vitest";
import { createComponent, createSignal } from "solid-js";
import { render } from "solid-js/web";
import { PhaseLayers, PhasePane } from "./PhasePane";
import { PhaseDraftEditor, type DraftSnapshot } from "./PhaseDraftEditor";
import type { DraftInput, PhaseFollow } from "../../store/team-phases";
import type { TeamOutcome, TeamView } from "../../store/teams";
import fixture from "./fixtures/phase-pane.json";

const disposers: (() => void)[] = [];
afterEach(() => { disposers.splice(0).forEach((d) => d()); document.body.replaceChildren(); vi.restoreAllMocks(); });
function host() { const h = document.createElement("div"); document.body.append(h); return h; }
function button(h: HTMLElement, text: string) { return [...h.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent === text)!; }
const view = () => structuredClone(fixture) as unknown as TeamView;
function input(h: HTMLElement, selector: string, value: string) {
  const field = h.querySelector<HTMLInputElement>(selector)!; field.value = value;
  field.dispatchEvent(new Event("input", { bubbles: true }));
}
async function pane(over?: (v: TeamView) => void) {
  const v = view(); over?.(v);
  const [read, setRead] = createSignal(v);
  const h = host(), follow = vi.fn<PhaseFollow>().mockResolvedValue(v.plan!.epoch);
  const submit = vi.fn<(s: DraftSnapshot, d: DraftInput) => Promise<TeamOutcome>>()
    .mockResolvedValue({ outcome: "done", receipt: {}, request_id: "example-request" });
  disposers.push(render(() => createComponent(PhasePane, { get view() { return read(); }, busy: false, follow, submit }), h));
  await vi.waitFor(() => expect(follow).toHaveBeenCalledTimes(2));
  return { h, follow, submit, setRead };
}

it("shows core layers and the original draft details without creating tasks or offering approval", async () => {
  const { h, submit } = await pane();
  expect(h.querySelectorAll("[data-phase-node]")).toHaveLength(2);
  expect(h.textContent).toContain("Layer 2");
  button(h, "Build the foundation").click();
  expect(h.textContent).toContain("Implement the shared part.");
  expect(h.textContent).toContain("The foundation tests pass");
  expect(h.querySelector<HTMLButtonElement>("[data-phase-approve]")!.disabled).toBe(true);
  expect(h.textContent).toContain("Phase approval is not available yet.");
  expect(submit).not.toHaveBeenCalled();
});

it("groups the provided layer numbers without recalculating dependency order", () => {
  const draft = view().plan!.draft!;
  // A projection fault/control: topology is unchanged, supplied layers differ.
  draft.graph.nodes[0].layer = 7;
  const h = host(); disposers.push(render(() => createComponent(PhaseLayers, { draft }), h));
  expect(h.querySelector('[aria-label="Layer 8"] [data-phase-node="1"]')).not.toBeNull();
});

it("keeps an unknown-field draft visible and read-only", async () => {
  const { h } = await pane((v) => { v.plan!.draft!.unknown_fields = true; v.plan!.draft!.editable = false; });
  expect(button(h, "Edit draft").disabled).toBe(true);
  expect(h.textContent).toContain("fields this window cannot edit");
  expect(h.querySelectorAll("[data-phase-node]")).toHaveLength(2);
});

it("shows a failed read and a working explicit reread instead of an empty plan", async () => {
  const { h, follow } = await pane((v) => { v.plan!.draft = null; v.plan!.draft_failure = { code: "not_found", message: "Older CLI has no phase command" }; });
  expect(h.textContent).toContain("Older CLI has no phase command");
  expect(h.querySelector("[data-phase-layers]")).toBeNull();
  expect(button(h, "New phase draft").disabled).toBe(true);
  button(h, "Read again").click();
  await vi.waitFor(() => expect(follow).toHaveBeenCalledTimes(3));
});

it("edits the bounded draft with its frozen read revision and generation", async () => {
  const { h, submit, setRead } = await pane();
  button(h, "Edit draft").click();
  input(h, "[data-phase-title]", "Edited foundation");
  input(h, "[data-node-criteria]", "A precise criterion\nA second criterion");
  const newer = view(); newer.revision = 900; newer.bar.revision = 900;
  setRead(newer);
  h.querySelector<HTMLButtonElement>("[data-save-phase]")!.click();
  await vi.waitFor(() => expect(submit).toHaveBeenCalledTimes(1));
  const [snapshot, draft] = submit.mock.calls[0];
  expect(snapshot.revision).toBe(2); expect(snapshot.generation).toBe(1);
  expect(draft.title).toBe("Edited foundation");
  expect(draft.nodes[0].criteria).toEqual(["A precise criterion", "A second criterion"]);
  expect(draft.nodes[1].deps).toEqual([{ node: 1 }]);
});

it("keeps a known validation refusal editable for an explicit corrected save", async () => {
  const { h, submit } = await pane();
  submit.mockResolvedValueOnce({ outcome: "failed", code: "over_cap", detail: "Too many write paths.", request_id: "example-request" });
  button(h, "Edit draft").click();
  const writes = [...h.querySelectorAll("label")].find((label) => label.textContent?.startsWith("Write paths, one per line"))!.querySelector("textarea")!;
  const change = (text: string) => { writes.value = text; writes.dispatchEvent(new Event("input", { bubbles: true })); };
  change(Array.from({ length: 205 }, (_, i) => `src/base-${i}.rs`).join("\n"));
  const save = h.querySelector<HTMLButtonElement>("[data-save-phase]")!;
  save.click();
  await vi.waitFor(() => expect(h.textContent).toContain("Too many write paths."));
  expect(save.disabled).toBe(false);
  expect(submit.mock.calls[0][1].nodes[0].writes).toHaveLength(205);
  change("src/base.rs\nsrc/base-tests.rs");
  save.click();
  await vi.waitFor(() => expect(submit).toHaveBeenCalledTimes(2));
  expect(submit.mock.calls[1][0].revision).toBe(2);
  expect(submit.mock.calls[1][0].generation).toBe(1);
  expect(submit.mock.calls[1][1].nodes[0].writes).toEqual(["src/base.rs", "src/base-tests.rs"]);
});

it.each(["pending", "uncertain", "stale_revision"])("preserves text and blocks a second save after %s", async (state) => {
  const h = host(), d = view().plan!.draft!;
  const submit = vi.fn().mockResolvedValue(state === "stale_revision"
    ? { outcome: "failed", code: state, detail: "The revision changed", request_id: "example-request" }
    : { outcome: state, detail: "Reply lost", request_id: "example-request" });
  const close = vi.fn();
  disposers.push(render(() => createComponent(PhaseDraftEditor, { snapshot: { phase: 1, revision: 2, generation: 1, next_node: 3,
    input: { title: "Foundation", barrier: true, nodes: d.nodes } }, routes: view().plan!.list!.routes, members: view().members, close, submit }), h));
  input(h, "[data-phase-title]", "Keep my text");
  const save = h.querySelector<HTMLButtonElement>("[data-save-phase]")!;
  save.click(); save.click();
  await vi.waitFor(() => expect(save.disabled).toBe(true));
  expect(submit).toHaveBeenCalledTimes(1); expect(close).not.toHaveBeenCalled();
  expect(h.querySelector<HTMLInputElement>("[data-phase-title]")!.value).toBe("Keep my text");
  expect(h.textContent).toContain(state === "stale_revision" ? "your text stays here to copy" : "use Check");
  save.click(); expect(submit).toHaveBeenCalledTimes(1);
});
