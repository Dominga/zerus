// @vitest-environment happy-dom
// Normal rows come from CLI writers through the core model/coalescer/batch.
import { afterEach, expect, it, vi } from "vitest";
import { createComponent } from "solid-js";
import { render } from "solid-js/web";
import fixture from "./fixtures/planning-start.json";
import { PlanningNotice } from "./PlanningStart";
import { barActions } from "./actions";
import type { TeamView } from "../../store/teams";

let dispose: (() => void) | undefined;
afterEach(() => { dispose?.(); dispose=undefined; document.body.replaceChildren(); vi.restoreAllMocks(); });

it("shows the real planning goal and offers only planning pause and stop", () => {
  const v=structuredClone(fixture.view) as unknown as TeamView;
  const host=document.createElement("div"); document.body.append(host);
  dispose=render(()=>createComponent(PlanningNotice,{status:v.bar.status,goal:v.bar.goal,planning:v.planning}),host);
  expect(host.textContent).toContain(fixture.view.planning.goal);
  expect(host.textContent).toContain("Other members and the integration branch wait");
  expect(barActions(v).map(a=>a.id)).toEqual(["pause","stop"]);
});

it("keeps unknown planning state visible without offering resume", () => {
  // An intentional projection fault, not a stored row or a claim of freedom.
  const v=structuredClone(fixture.view) as unknown as TeamView;
  v.planning={...v.planning!,known:false,paused:false};
  const host=document.createElement("div"); document.body.append(host);
  dispose=render(()=>createComponent(PlanningNotice,{status:v.bar.status,goal:v.bar.goal,planning:v.planning}),host);
  expect(host.textContent).toContain("Planning state could not be confirmed");
  expect(host.textContent).toContain(fixture.view.planning.goal);
  expect(barActions(v).map(a=>a.id)).toEqual(["stop"]);
});
