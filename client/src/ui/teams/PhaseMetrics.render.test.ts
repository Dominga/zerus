// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from "vitest";
import { createComponent, createSignal } from "solid-js";
import { render } from "solid-js/web";
import { PhaseMetrics } from "./PhaseMetrics";
import { PhasePane } from "./PhasePane";
import type { PhaseMetricsView } from "../../store/phase-metrics";
import type { TeamView } from "../../store/teams";
import fixture from "./fixtures/phase-metrics.json";

const disposers: (() => void)[] = [];
afterEach(() => { disposers.splice(0).forEach((dispose) => dispose()); document.body.replaceChildren(); vi.restoreAllMocks(); });
const product = () => structuredClone(fixture) as unknown as TeamView;
const metric = () => product().plan!.draft!.metrics!;
function mount(metrics: PhaseMetricsView | undefined = metric()) {
  const host = document.createElement("div"); document.body.append(host);
  const [failed, setFailed] = createSignal(false);
  disposers.push(render(() => createComponent(PhaseMetrics, { metrics, get readFailed() { return failed(); } }), host));
  return { host, setFailed };
}

it("shows operation-written path and parallelism as separate core-projected chips", () => {
  const { host } = mount();
  expect(host.querySelector('[data-phase-path]')!.textContent).toBe('Path 45 min');
  expect(host.querySelector('[data-phase-parallelism]')!.textContent).toBe('Parallelism 1.4');
  expect(host.querySelector('[data-phase-path]')!.parentElement).toBe(host.querySelector('[data-phase-parallelism]')!.parentElement);
  expect(host.querySelector('[data-phase-warnings]')).toBeNull();
  expect(host.textContent).not.toContain(String.fromCodePoint(0x00b7));
});

it("mounts the core metrics in the actual phase header without sending any action", async () => {
  const view = product(), host = document.createElement('div'); document.body.append(host);
  const follow = vi.fn().mockResolvedValue(view.plan!.epoch), submit = vi.fn();
  disposers.push(render(() => createComponent(PhasePane, { view, busy: false, follow, submit }), host));
  await vi.waitFor(() => expect(host.querySelector('.phase-header [data-phase-path]')?.textContent).toBe('Path 45 min'));
  expect(submit).not.toHaveBeenCalled();
  expect(host.querySelector('[data-phase-warnings]')).toBeNull();
});

it("suppresses a previously good metric while the current phase read has failed", () => {
  const { host, setFailed } = mount();setFailed(true);
  expect(host.querySelector('[data-phase-path]')!.textContent).toBe('Path unavailable');
  expect(host.querySelector('[data-phase-parallelism]')!.textContent).toBe('Parallelism unavailable');
  expect(host.textContent).toContain('read has failed');
  expect(host.textContent).not.toContain('45 min');
});

it("the actual phase header suppresses retained metrics on a failed current read", async () => {
  const initial=product(), host=document.createElement('div');document.body.append(host);
  const [view,setView]=createSignal(initial), follow=vi.fn().mockResolvedValue(initial.plan!.epoch), submit=vi.fn();
  disposers.push(render(()=>createComponent(PhasePane,{get view(){return view();},busy:false,follow,submit}),host));
  await vi.waitFor(()=>expect(host.querySelector('.phase-header [data-phase-path]')?.textContent).toBe('Path 45 min'));
  setView({...initial,plan:{...initial.plan!,draft_failure:{code:'unavailable',message:''}}});
  expect(host.querySelector('.phase-header [data-phase-path]')!.textContent).toBe('Path unavailable');
  expect(host.querySelector('.phase-header [data-phase-metric-reason]')!.textContent).toContain('read has failed');
  expect(submit).not.toHaveBeenCalled();
});

// Synthetic view fault controls: these change availability flags and optional
// presentation findings on the product fixture, never persisted business rows.
it("keeps incomplete, unknown-estimate and unsupported live snapshots visibly unavailable", () => {
  for (const unavailable of ['incomplete_phase', 'missing_estimates', 'live_snapshot_unavailable', 'unknown_fields'] as const) {
    const m = metric();m.unavailable = unavailable;m.critical_path_minutes = null;m.average_parallelism_tenths = null;
    const { host } = mount(m);
    expect(host.querySelector('[data-phase-path]')!.textContent).toBe('Path unavailable');
    expect(host.querySelector('[data-phase-parallelism]')!.textContent).toBe('Parallelism unavailable');
    expect(host.querySelector('[data-phase-metric-reason]')!.textContent).not.toBe('');
    expect(host.querySelector('[data-phase-warnings]')).toBeNull();
  }
});

it("keeps an older core with no metrics unavailable", () => {
  const host=document.createElement('div');document.body.append(host);
  disposers.push(render(()=>createComponent(PhaseMetrics,{}),host));
  expect(host.textContent).toContain('Path unavailable');
  expect(host.textContent).toContain('complete phase has not been read');
});

it("names the intrinsic phase scope when outside prerequisites are present", () => {
  const m=metric();m.external_dependencies=1;const {host}=mount(m);
  expect(host.textContent).toContain('Waits outside this phase are excluded.');
  expect(host.querySelector('[data-phase-path]')!.textContent).toBe('Path 45 min');
});

it("formats the supplied core values without substituting a renderer calculation", () => {
  const m=metric();m.critical_path_minutes=90;m.average_parallelism_tenths=22;const {host}=mount(m);
  expect(host.querySelector('[data-phase-path]')!.textContent).toBe('Path 90 min');
  expect(host.querySelector('[data-phase-parallelism]')!.textContent).toBe('Parallelism 2.2');
});

it("renders finding-specific details and closes the native disclosure with Escape or outside click", () => {
  const m=metric();m.warnings=['Existing non-fatal finding for task #1','Another existing non-fatal finding for task #2'];
  const { host }=mount(m);const details=host.querySelector<HTMLDetailsElement>('details')!;
  expect(host.querySelector('summary')!.textContent).toBe('2 plan warnings');
  expect([...host.querySelectorAll('li')].map((li)=>li.textContent)).toEqual(m.warnings);
  details.open=true;details.dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true}));expect(details.open).toBe(false);
  details.open=true;document.body.dispatchEvent(new PointerEvent('pointerdown',{bubbles:true}));expect(details.open).toBe(false);
});
