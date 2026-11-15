// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from "vitest";
import { createComponent, createSignal } from "solid-js";
import { render } from "solid-js/web";
import { PlanView } from "./PlanView";
import { PlanToolbar } from "./PlanToolbar";
import { createTeamPlanPreferences } from "../../store/teamPlan";
import type { TeamView } from "../../store/teams";
import graph from "../../../core/src/teams/fixtures/graph-view.json";
import held from "../../../core/src/teams/fixtures/board-pending.json";

let dispose: (() => void) | undefined;
afterEach(() => { dispose?.(); document.body.replaceChildren(); vi.restoreAllMocks(); });
function mount(initial: TeamView = graph as unknown as TeamView, preferences = createTeamPlanPreferences()) {
  const host = document.createElement("div"); document.body.append(host);
  const [view, setView] = createSignal(initial), act = vi.fn();
  dispose = render(() => createComponent(PlanView, { get view() { return view(); }, busy: false, selected: null, preferences, onSelect: vi.fn(), onSetDeps: act }), host);
  const click = (selector: string) => host.querySelector<HTMLButtonElement>(selector)!.click();
  return { host, click, preferences, setView, act };
}

it("filters the core-written held queue fixture", () => {
  const { host, click, act } = mount(held as unknown as TeamView);
  click('[data-plan-filter="problems"]');
  const rows = (held as unknown as TeamView).groups.flatMap((group) => group.cards).filter((card) => card.held);
  expect([...host.querySelectorAll('[data-plan-task]')].map((node) => Number(node.getAttribute('data-plan-task')))).toEqual(rows.map((row) => row.node));
  expect(rows).toHaveLength(1);
  expect(host.querySelector('[data-plan-problem="true"]')).not.toBeNull();
  click('[data-plan-filter="active"]');
  expect(host.querySelectorAll('[data-plan-task]')).toHaveLength(5);
  expect(act).not.toHaveBeenCalled();
});

it("zooms cards and arrows together, Fits and resets to 100 percent without an action", () => {
  const { host, click, act } = mount();
  click('[aria-label="Zoom in"]');
  const canvas = () => host.querySelector<HTMLElement>('.team-plan-scaled')!;
  expect(canvas().style.transform).toBe('scale(1.1)');
  expect(canvas().querySelector('svg')).not.toBeNull();
  const space = host.querySelector<HTMLElement>('.team-plan-scale-space')!;
  expect(parseFloat(space.style.width)).toBeCloseTo(parseFloat(canvas().style.width) * 1.1);
  vi.spyOn(host.querySelector('[data-team-graph]')!, 'clientWidth', 'get').mockReturnValue(500);
  [...host.querySelectorAll<HTMLButtonElement>('button')].find((button) => button.textContent === 'Fit')!.click();
  expect(Number(host.querySelector('[data-plan-zoom]')!.getAttribute('data-plan-zoom'))).toBeLessThan(1);
  click('[aria-label="Reset zoom"]');
  expect(canvas().style.transform).toBe('scale(1)');
  expect(act).not.toHaveBeenCalled();
});

it("collapses and expands sections and retains choices across team switches and remounts", () => {
  const { host, click, preferences, setView, act } = mount();
  click('[data-plan-fold-all]');
  expect(host.querySelectorAll('[data-plan-task]')).toHaveLength(0);
  expect(host.querySelector('[data-plan-section]')!.getAttribute('aria-expanded')).toBe('false');
  expect(host.textContent).toContain('Expand all');
  setView({ ...graph as unknown as TeamView, bar: { ...graph.bar, id: 'another-team' } });
  expect(host.querySelectorAll('[data-plan-task]')).toHaveLength(3);
  setView(graph as unknown as TeamView);
  expect(host.querySelectorAll('[data-plan-task]')).toHaveLength(0);
  click('[data-plan-fold-all]');
  click('[data-plan-section]');
  click('[data-plan-filter="problems"]');
  expect(host.textContent).toContain('No tasks on this page match');
  expect(act).not.toHaveBeenCalled();
  dispose?.();
  const again = mount(graph as unknown as TeamView, preferences);
  expect(again.host.querySelector('[data-plan-filter="problems"]')!.getAttribute('aria-checked')).toBe('true');
});

it("clears an invisible selected arrow before Delete can edit it", () => {
  const { host, click, act } = mount();
  host.querySelector('[data-edge="2/3"]')!.dispatchEvent(new MouseEvent('click', { bubbles: true }));
  click('[data-plan-filter="active"]');
  host.querySelector('[data-team-graph]')!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Delete', bubbles: true }));
  expect(act).not.toHaveBeenCalled();
  expect(host.textContent).toContain('current queue page');
});

it("binds both visibility toggles to the team's viewer state without changing queue data", () => {
  const before = JSON.stringify(graph);
  const { host, click, preferences, act } = mount();
  click('[data-plan-toggle="cancelled"]'); click('[data-plan-toggle="finished"]');
  expect(host.querySelector('[data-plan-toggle="cancelled"]')!.getAttribute('aria-pressed')).toBe('true');
  expect(host.querySelector('[data-plan-toggle="finished"]')!.getAttribute('aria-pressed')).toBe('true');
  expect(preferences.get(graph.bar.id)).toMatchObject({ showCancelled: true, hideFinished: true });
  expect(JSON.stringify(graph)).toBe(before);
  expect(act).not.toHaveBeenCalled();
});

// Synthetic optional-wire-field control: the base rows are the product graph
// fixture; only these origin values are written by hand for decoder coverage.
it("shows optional origins as read-only links and names an off-page origin", () => {
  const view = structuredClone(graph) as unknown as TeamView;
  const cards = view.groups.flatMap((group) => group.cards);
  cards.find((card) => card.node === 2)!.origin = 1;
  cards.find((card) => card.node === 3)!.origin = 99;
  const { host, act } = mount(view);
  expect(host.querySelector('[data-plan-origin="1/2"]')).not.toBeNull();
  expect(host.querySelectorAll('[data-plan-origin]')).toHaveLength(1);
  expect(host.textContent).toContain('Spawned from #99');
  expect(host.querySelector('[data-plan-origin]')!.getAttribute('role')).toBeNull();
  expect(act).not.toHaveBeenCalled();
});

it("mounts without Graph controls for Layers and omits unsupported origin and unrelated modes", () => {
  const host = document.createElement('div'); document.body.append(host);
  const preferences = createTeamPlanPreferences();
  dispose = render(() => createComponent(PlanToolbar, { state: preferences.get('a'), onChange: vi.fn() }), host);
  expect(host.querySelector('[data-plan-filter="active"]')).not.toBeNull();
  for (const text of ['Spawned from', 'Heat', 'Auto-accept drafts', 'Lanes', 'Profile', 'Tests', 'Builds', 'Zoom in', 'Fit']) expect(host.textContent).not.toContain(text);
});
