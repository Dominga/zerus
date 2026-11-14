// @vitest-environment happy-dom
// Rows are the real CLI queue writer and core projection's pinned graph view.
import { afterEach, expect, it, vi } from "vitest";
import { createComponent, createSignal } from "solid-js";
import { render } from "solid-js/web";
import { PlanView } from "./PlanView";
import { TeamWorkspace } from "./TeamTab";
import type { TeamDependency, TeamView } from "../../store/teams";
import graphView from "../../../core/src/teams/fixtures/graph-view.json";
import { teamView } from "./fixtures";

let dispose: (() => void) | undefined;
afterEach(() => { dispose?.(); dispose = undefined; document.body.replaceChildren(); vi.restoreAllMocks(); });
function setup(options: { busy?: boolean; view?: TeamView } = {}) {
  const host = document.createElement("div"); document.body.append(host);
  const calls: [number, TeamDependency[]][] = [];
  const [view, setView] = createSignal(options.view ?? graphView as unknown as TeamView);
  dispose = render(() => createComponent(PlanView, {
    get view() { return view(); }, busy: options.busy ?? false, selected: null, onSelect: vi.fn(), onSetDeps: (node, deps) => calls.push([node, deps]),
  }), host);
  const button = (selector: string) => host.querySelector<HTMLButtonElement>(selector)!;
  return { host, calls, setView, button };
}

it("uses core layers, draws arrows and lists a dependency outside this queue page", () => {
  const { host } = setup();
  expect(host.querySelectorAll("[data-plan-task]")).toHaveLength(3);
  expect(host.querySelector('[data-edge="1/2"]')?.classList.contains("soft")).toBe(true);
  expect(host.querySelector('[data-edge="2/3"]')).not.toBeNull();
  expect(host.querySelector('[data-plan-task="3"]')?.textContent).toContain("#4");
  expect(host.textContent).toContain("Layer 3");
  expect(host.textContent).toContain("current queue page");
});

it("links ports by keyboard-compatible clicks, preserving off-page and soft dependencies", () => {
  const { button, calls } = setup();
  button('[data-plan-task="1"] .output').click();
  button('[data-plan-task="3"] .input').click();
  expect(calls).toEqual([[3, [{ node: 2 }, { node: 4 }, { node: 1 }]]]);
});

it("links by dragging the output port onto the target card", () => {
  const { host, button, calls } = setup();
  const target = host.querySelector('[data-plan-task="3"]')!;
  vi.spyOn(document, "elementFromPoint").mockReturnValue(target);
  button('[data-plan-task="1"] .output').dispatchEvent(new PointerEvent("pointerdown", { button: 0, bubbles: true }));
  window.dispatchEvent(new PointerEvent("pointerup", { clientX: 10, clientY: 10 }));
  expect(calls).toEqual([[3, [{ node: 2 }, { node: 4 }, { node: 1 }]]]);
});

it("selects an arrow without mutation and Delete removes only that edge", () => {
  const { host, calls } = setup();
  host.querySelector('[data-edge="2/3"]')!.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  expect(calls).toEqual([]);
  host.querySelector('[data-team-graph]')!.dispatchEvent(new KeyboardEvent("keydown", { key: "Delete", bubbles: true }));
  expect(calls).toEqual([[3, [{ node: 4 }]]]);
});

it("keeps arrows read-only when busy, decided or the team is stopped", () => {
  for (const state of ["busy", "decided", "stopped"]) {
    const view = structuredClone(graphView) as unknown as TeamView;
    if (state === "stopped") view.bar.status = "stopped";
    if (state === "decided") view.groups.forEach((group) => group.cards.forEach((card) => card.state = "accepted"));
    const { host, calls } = setup({ view, busy: state === "busy" });
    host.querySelector('[data-edge="2/3"]')!.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    host.querySelector('[data-team-graph]')!.dispatchEvent(new KeyboardEvent("keydown", { key: "Delete", bubbles: true }));
    expect(calls).toEqual([]);
    dispose?.(); dispose = undefined; document.body.replaceChildren();
  }
});

it("cancels selected edges and linking when a newer revision is displayed", () => {
  const { host, button, setView, calls } = setup();
  host.querySelector('[data-edge="2/3"]')!.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  setView({ ...graphView as unknown as TeamView, revision: 1000 });
  host.querySelector('[data-team-graph]')!.dispatchEvent(new KeyboardEvent("keydown", { key: "Delete", bubbles: true }));
  button('[data-plan-task="1"] .output').click();
  setView({ ...graphView as unknown as TeamView, revision: 1001 });
  expect(button('[data-plan-task="3"] .input').disabled).toBe(true);
  expect(calls).toEqual([]);
});

it("offers Graph beside Board and preserves the existing task detail", () => {
  const host = document.createElement("div"); document.body.append(host);
  dispose = render(() => createComponent(TeamWorkspace, { view: teamView, busy: false, failure: null, onAction: vi.fn(), onPage: vi.fn(), onAddTask: vi.fn() }), host);
  expect(host.querySelector("[data-team-board]")).not.toBeNull();
  host.querySelector<HTMLButtonElement>("[data-team-graph-mode]")!.click();
  expect(host.querySelector("[data-team-graph]")).not.toBeNull();
  host.querySelector<HTMLButtonElement>(".team-plan-card")!.click();
  expect(host.querySelector("[data-team-detail]")).not.toBeNull();
});

it("the Team tab sends dependency edits through its ordinary action callback", () => {
  const host = document.createElement("div"); document.body.append(host);
  const actions: [string, Record<string, unknown> | undefined][] = [];
  dispose = render(() => createComponent(TeamWorkspace, { view: graphView as unknown as TeamView, busy: false, failure: null,
    onAction: (kind, fields) => actions.push([kind, fields]), onPage: vi.fn(), onAddTask: vi.fn() }), host);
  host.querySelector<HTMLButtonElement>("[data-team-graph-mode]")!.click();
  host.querySelector<HTMLButtonElement>('[data-plan-task="1"] .output')!.click();
  host.querySelector<HTMLButtonElement>('[data-plan-task="3"] .input')!.click();
  expect(actions).toEqual([["set_deps", { node: 3, deps: [{ node: 2 }, { node: 4 }, { node: 1 }] }]]);
});
