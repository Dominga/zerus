// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from "vitest";
import { createComponent, createSignal } from "solid-js";
import { render } from "solid-js/web";
import { TeamWorkspace } from "./TeamTab";
import { openTeamNode, setOpenTeamNode } from "./openNode";
import { teamPlanPreferences } from "../../store/ui";
import type { TeamView } from "../../store/teams";
import fixture from "./fixtures/lanes-view.json";
import graphView from "../../../core/src/teams/fixtures/graph-view.json";

// Cards, members and queue pages come from the Rust operation/view writers.
// Extra input/overlay elements below are only keyboard ownership controls.
const data = fixture as unknown as { before: TeamView; after: TeamView; pages: TeamView[] };
let dispose: (() => void) | undefined;
afterEach(() => {
  dispose?.(); dispose = undefined;
  setOpenTeamNode(null);
  document.body.replaceChildren();
  teamPlanPreferences.update(data.before.bar.id, { filter: "all" });
});
function setup(view = data.before) {
  const [current, setView] = createSignal(view);
  const host = document.createElement("div"); document.body.append(host);
  const action = vi.fn(), page = vi.fn(), save = vi.fn(), add = vi.fn();
  dispose = render(() => createComponent(TeamWorkspace, {
    get view() { return current(); }, busy: false, failure: null,
    onAction: action, onPage: page, onAddTask: add, onSettingsSave: save,
  }), host);
  const select = (node = 1) => host.querySelector<HTMLButtonElement>(`.team-task-card[data-node="${node}"]`)!.click();
  const close = () => host.querySelector<HTMLButtonElement>('[data-team-detail] button[aria-label="Close"]');
  const quiet = () => { expect(action).not.toHaveBeenCalled(); expect(save).not.toHaveBeenCalled(); expect(add).not.toHaveBeenCalled(); };
  return { host, setView, select, close, action, page, quiet };
}
const mode = (host: HTMLElement, label: string) => [...host.querySelectorAll<HTMLButtonElement>("button")].find((button) => button.textContent === label)!;
function escape(target: Element, init: KeyboardEventInit = {}) {
  const event = new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true, ...init });
  target.dispatchEvent(event); return event;
}
function dismissed(host: HTMLElement) {
  expect(host.querySelector("[data-team-detail]")).toBeNull();
  expect(host.querySelector('.team-task-selected, .team-plan-card[aria-pressed="true"], [data-lane-task][aria-pressed="true"]')).toBeNull();
}

it.each(["Board", "Graph", "Lanes"])("Close clears the shared %s selection without a task operation or page read", (kind) => {
  const { host, select, close, quiet, page } = setup();
  select(); mode(host, kind).click();
  expect(host.querySelector("[data-team-detail] h3")?.textContent).toBe("Task 1");
  expect(close()?.type).toBe("button"); expect(close()?.querySelector("svg")).not.toBeNull();
  close()!.click(); dismissed(host);
  expect(mode(host, kind).getAttribute("aria-pressed")).toBe("true");
  quiet(); expect(page).not.toHaveBeenCalled();
});

it("empty board and column space dismiss, while nested card text and normal task actions work", () => {
  const { host, select, action, quiet, page } = setup();
  host.querySelector<HTMLElement>('.team-task-card[data-node="1"] .team-task-title')!.click();
  expect(host.querySelector("[data-team-detail]")).not.toBeNull(); quiet();
  host.querySelector<HTMLElement>("[data-team-board]")!.click(); dismissed(host); quiet();
  select(); host.querySelector<HTMLElement>(".team-column-empty")!.click(); dismissed(host); quiet();
  select();
  const accept = host.querySelector<HTMLButtonElement>('[data-task-action="accept"]')!;
  expect(accept.disabled).toBe(false); accept.click();
  expect(action).toHaveBeenCalledExactlyOnceWith("accept", { node: 1 });
  host.querySelector<HTMLButtonElement>('[data-task-action="return"]')!.click();
  const input = host.querySelector<HTMLInputElement>(".team-reason input")!;
  input.value = "Please add coverage"; input.dispatchEvent(new Event("input", { bubbles: true }));
  mode(host, "Send").click();
  expect(action).toHaveBeenLastCalledWith("return", { node: 1, reason: "Please add coverage" });
  expect(page).not.toHaveBeenCalled();
});

it("Escape is local, clears the selection and discards an unsent reason without submitting it", () => {
  const { host, select, quiet, page } = setup(); select();
  host.querySelector<HTMLButtonElement>('[data-task-action="return"]')!.click();
  const input = host.querySelector<HTMLInputElement>(".team-reason input")!;
  input.value = "Do not send"; input.dispatchEvent(new Event("input", { bubbles: true }));
  escape(document.body); expect(host.querySelector("[data-team-detail]")).not.toBeNull();
  const event = escape(host.querySelector("[data-team-detail] h3")!);
  expect(event.defaultPrevented).toBe(true); dismissed(host);
  select(); expect(host.querySelector(".team-reason")).toBeNull();
  quiet(); expect(page).not.toHaveBeenCalled();
});

it.each(["input", "textarea", "select", "editable", "textbox"])("Escape belongs to the %s editor", (kind) => {
  const { host, select, quiet } = setup(); select();
  const editor = document.createElement(kind === "editable" || kind === "textbox" ? "div" : kind);
  if (kind === "editable") editor.setAttribute("contenteditable", "plaintext-only");
  if (kind === "textbox") editor.setAttribute("role", "textbox");
  host.querySelector("[data-team-detail]")!.append(editor);
  expect(escape(editor).defaultPrevented).toBe(false);
  expect(host.querySelector("[data-team-detail]")).not.toBeNull(); quiet();
});

it.each(["dialog", "menu"])("an open %s owns Escape even when focus remains on the board", (kind) => {
  const { host, select, quiet } = setup(); select();
  const overlay = document.createElement(kind === "dialog" ? "dialog" : "div");
  if (kind === "dialog") overlay.setAttribute("open", ""); else overlay.setAttribute("role", "menu");
  document.body.append(overlay);
  expect(escape(host.querySelector("[data-team-detail] h3")!).defaultPrevented).toBe(false);
  expect(host.querySelector("[data-team-detail]")).not.toBeNull(); quiet();
});

it("a popover inside a closed native dialog does not steal the preview's Escape", () => {
  const { host, select, quiet } = setup(); select();
  const closed = document.createElement("dialog"), popover = document.createElement("div");
  popover.setAttribute("role", "dialog"); closed.append(popover); document.body.append(closed);
  escape(host.querySelector("[data-team-detail] h3")!); dismissed(host); quiet();
});

it("does not take an already handled or composing Escape", () => {
  const { host, select, quiet } = setup(); select();
  const target = host.querySelector("[data-team-detail] h3")!;
  target.addEventListener("keydown", (event) => event.preventDefault(), { once: true });
  escape(target); expect(host.querySelector("[data-team-detail]")).not.toBeNull();
  escape(target, { isComposing: true }); expect(host.querySelector("[data-team-detail]")).not.toBeNull(); quiet();
});

it("the graph cancels its own dependency gesture before Escape can dismiss the preview", () => {
  const { host, select, quiet } = setup(); select(); mode(host, "Graph").click();
  const graph = host.querySelector<HTMLElement>("[data-team-graph]")!;
  graph.querySelector<HTMLButtonElement>(".team-plan-port.output")!.click();
  expect(graph.querySelector('[role="status"] button')).not.toBeNull();
  escape(graph);
  expect(graph.querySelector('[role="status"] button')).toBeNull();
  expect(host.querySelector("[data-team-detail]")).not.toBeNull(); quiet();
  escape(graph); dismissed(host); quiet();
});

it("Escape clears a selected dependency arrow before dismissing the preview", () => {
  const { host, select, quiet } = setup(graphView as unknown as TeamView); select(); mode(host, "Graph").click();
  const graph = host.querySelector<HTMLElement>("[data-team-graph]")!;
  graph.querySelector('[data-edge="2/3"]')!.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  expect(graph.querySelector(".team-plan-edge.selected")).not.toBeNull();
  escape(graph); expect(graph.querySelector(".team-plan-edge.selected")).toBeNull();
  expect(host.querySelector("[data-team-detail]")).not.toBeNull(); quiet();
  escape(graph); dismissed(host); quiet();
});

it.each(["close", "escape", "blank"])("%s cancels a pending Open task lookup, ignores its late page, and allows a fresh Open task", (how) => {
  const { host, select, close, setView, page, quiet } = setup(data.pages[1]); select();
  setOpenTeamNode({ team: data.before.bar.id, node: 5 });
  expect(page).toHaveBeenCalledTimes(1); expect(openTeamNode()).not.toBeNull();
  if (how === "close") close()!.click();
  else if (how === "escape") escape(host.querySelector("[data-team-detail] h3")!);
  else host.querySelector<HTMLElement>("[data-team-board]")!.click();
  dismissed(host); expect(openTeamNode()).toBeNull();
  setView(data.pages[0]); dismissed(host); expect(page).toHaveBeenCalledTimes(1);
  setView(data.after); dismissed(host); quiet();
  setOpenTeamNode({ team: data.before.bar.id, node: 5 });
  expect(host.querySelector("[data-team-detail] h3")?.textContent).toBe(data.after.groups.flatMap((group) => group.cards).find((card) => card.node === 5)!.title);
  expect(openTeamNode()).toBeNull(); quiet();
});

it("dismissal and same-team refresh keep the canvas, scroll, mode and graph filter", () => {
  const { host, select, close, setView, quiet } = setup(); select(); mode(host, "Graph").click();
  teamPlanPreferences.update(data.before.bar.id, { filter: "active" });
  const canvas = host.querySelector<HTMLElement>(".team-canvas")!;
  canvas.scrollTop = 180; canvas.scrollLeft = 45;
  close()!.click(); setView(data.after);
  expect(host.querySelector(".team-canvas")).toBe(canvas);
  expect([canvas.scrollTop, canvas.scrollLeft]).toEqual([180, 45]);
  expect(mode(host, "Graph").getAttribute("aria-pressed")).toBe("true");
  expect(teamPlanPreferences.get(data.before.bar.id).filter).toBe("active");
  dismissed(host); quiet();
});
