// @vitest-environment happy-dom
import { afterEach, expect, it } from "vitest";
import { createComponent } from "solid-js";
import { render } from "solid-js/web";
import { TaskDependencies } from "./TaskDependencies";
import type { TeamCard, TeamDependency, TeamView } from "../../store/teams";
import graphView from "../../../core/src/teams/fixtures/graph-view.json";
let dispose: (() => void) | undefined;
afterEach(() => { dispose?.(); document.body.replaceChildren(); });
function setup(busy = false, overlay: Partial<TeamCard> = {}) {
  const task = { ...(graphView as unknown as TeamView).groups.flatMap((group) => group.cards).find((task) => task.node === 3)!, ...overlay };
  const host = document.createElement("div"); document.body.append(host);
  const calls: TeamDependency[][] = [];
  dispose = render(() => createComponent(TaskDependencies, { task, status: "running", busy, onSetDeps: (deps) => calls.push(deps) }), host);
  return { host, calls };
}
it("can remove an off-page dependency while preserving the other edges", () => {
  const { host, calls } = setup();
  host.querySelector<HTMLButtonElement>('[aria-label="Remove dependency on #4"]')!.click();
  expect(calls).toEqual([[{ node: 2 }]]);
});
it("can add an off-page dependency by number and refuses duplicates and self", () => {
  const { host, calls } = setup();
  const input = host.querySelector<HTMLInputElement>("input")!;
  const button = host.querySelector<HTMLButtonElement>('[type="submit"]')!;
  for (const value of ["2", "3", "0", "1.5", "4294967296"]) {
    input.value = value; input.dispatchEvent(new Event("input", { bubbles: true })); expect(button.disabled).toBe(true);
  }
  input.value = "99"; input.dispatchEvent(new Event("input", { bubbles: true }));
  button.click();
  expect(calls).toEqual([[{ node: 2 }, { node: 4 }, { node: 99 }]]);
});
it("keeps a busy task's dependency list read-only", () => {
  const { host } = setup(true);
  expect(host.textContent).toContain("#4");
  expect(host.querySelector("button,input")).toBeNull();
});

it("shows why a pending task with stored attempt or effect history is read-only", () => {
  const { host, calls } = setup(false, { state: "pending", deps_editable: false });
  expect(host.querySelector("[data-dependency-edit-cause]")?.textContent).toBe("Dependencies can only be edited before this task starts or has an attempt or hand-out recorded.");
  expect(host.querySelector("button,input")).toBeNull();
  expect(calls).toEqual([]);
});
it("requires an authoritative eligibility fact from the task read", () => {
  const { host } = setup(false, { deps_editable: undefined });
  expect(host.querySelector("[data-dependency-edit-cause]")?.textContent).toContain("attempt history is known");
  expect(host.querySelector("button,input")).toBeNull();
});
