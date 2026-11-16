// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from "vitest";
import { createComponent, createSignal } from "solid-js";
import { render } from "solid-js/web";
import { TeamWorkspace } from "./TeamTab";
import { workspaceLayoutView } from "./fixtures/layout";

let dispose: (() => void) | undefined;
const observers = new Set<{ callback: ResizeObserverCallback; target?: Element }>();
afterEach(() => { dispose?.(); dispose = undefined; observers.clear(); vi.unstubAllGlobals(); document.body.replaceChildren(); });
const flush = async () => { await Promise.resolve(); await Promise.resolve(); };
function setup(width = 1100) {
  vi.stubGlobal("ResizeObserver", class {
    target?: Element;
    constructor(public callback: ResizeObserverCallback) { observers.add(this); }
    observe(target: Element) { this.target = target; this.callback([{ target, contentRect: { width } } as ResizeObserverEntry], this as unknown as ResizeObserver); }
    disconnect() { observers.delete(this); }
  });
  const host = document.createElement("div"); document.body.append(host);
  const action = vi.fn(), page = vi.fn(), save = vi.fn();
  const [view, setView] = createSignal(workspaceLayoutView());
  dispose = render(() => createComponent(TeamWorkspace, {
    get view() { return view(); }, busy: false, failure: null,
    onAction: action, onPage: page, onAddTask() {}, onSettingsSave: save,
  }), host);
  const select = () => { const card = host.querySelector<HTMLButtonElement>('.team-task-card[data-node="1"]')!; card.focus(); card.click(); return card; };
  const close = () => host.querySelector<HTMLButtonElement>('[data-team-detail] button[aria-label="Close"]')!;
  const resize = (next: number) => { for (const observer of observers) if (observer.target) observer.callback([{ target: observer.target, contentRect: { width: next } } as ResizeObserverEntry], observer as unknown as ResizeObserver); };
  const quiet = () => { expect(action).not.toHaveBeenCalled(); expect(page).not.toHaveBeenCalled(); expect(save).not.toHaveBeenCalled(); };
  return { host, select, close, resize, quiet, setView };
}
const escape = (target: Element) => target.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));

it("reuses the Members allocation for task details and restores Members on Close", () => {
  const { host, select, close, quiet } = setup(); select();
  const detail = host.querySelector("[data-team-detail]")!;
  expect(detail.closest("[data-team-inspector]")).not.toBeNull();
  expect(host.querySelector(".team-canvas")!.contains(detail)).toBe(false);
  expect(host.querySelector<HTMLElement>("[data-team-members]")!.hidden).toBe(true);
  close().click(); expect(host.querySelector("[data-team-detail]")).toBeNull();
  expect(host.querySelector<HTMLElement>("[data-team-members]")!.hidden).toBe(false); quiet();
});

it("narrow opening and Close return focus without replacing or resetting the Board", async () => {
  const { host, select, close, quiet } = setup(650);
  const board = host.querySelector<HTMLElement>("[data-team-board]")!;
  board.scrollLeft = 150; board.scrollTop = 70;
  const trigger = select(); await flush();
  const pane = host.querySelector<HTMLElement>("[data-team-inspector]")!;
  expect(pane.getAttribute("role")).toBe("dialog"); expect(pane.getAttribute("aria-modal")).not.toBe("true");
  expect(document.activeElement).toBe(close()); close().click(); await flush();
  expect(document.activeElement).toBe(trigger); expect(host.querySelector("[data-team-board]")).toBe(board);
  expect([board.scrollLeft, board.scrollTop]).toEqual([150, 70]); quiet();
});

it("Members remain reachable without a selected task and Escape closes their narrow overlay", async () => {
  const { host, quiet } = setup(650);
  const trigger = host.querySelector<HTMLButtonElement>("[data-team-members-toggle]")!;
  expect(trigger?.type).toBe("button"); trigger.focus(); trigger.click(); await flush();
  const pane = host.querySelector<HTMLElement>("[data-team-inspector]")!;
  expect(pane.getAttribute("role")).toBe("dialog");
  expect(host.querySelectorAll("[data-team-members] .team-member-card").length).toBeGreaterThan(0);
  const close = host.querySelector<HTMLButtonElement>('[data-team-members] button[aria-label="Close"]')!;
  expect(document.activeElement).toBe(close); escape(close); await flush();
  expect(pane.hasAttribute("data-team-inspector-overlay")).toBe(false);
  expect(document.activeElement).toBe(trigger); quiet();
});

it("the narrow inspector can go back to Members without adding a sidebar", async () => {
  const { host, select, quiet } = setup(650); select(); await flush();
  host.querySelector<HTMLButtonElement>("[data-team-inspector-members]")!.click(); await flush();
  expect(host.querySelector("[data-team-detail]")).toBeNull();
  expect(host.querySelector<HTMLElement>("[data-team-members]")!.hidden).toBe(false);
  expect(host.querySelectorAll("[data-team-inspector]")).toHaveLength(1);
  expect(host.querySelector("[data-team-inspector]")!.hasAttribute("data-team-inspector-overlay")).toBe(true); quiet();
});

it("resize preserves an edited reason and its focus while input Escape stays local", async () => {
  const { host, select, resize, quiet } = setup(650); select(); await flush();
  host.querySelector<HTMLButtonElement>('[data-task-action="return"]')!.click();
  const input = host.querySelector<HTMLInputElement>(".team-reason input")!;
  input.value = "Unsent context"; input.dispatchEvent(new Event("input", { bubbles: true })); input.focus();
  resize(1100); await flush(); expect(document.activeElement).toBe(input);
  resize(650); await flush(); expect(document.activeElement).toBe(input);
  escape(input); expect(host.querySelector("[data-team-detail]")).not.toBeNull();
  expect(input.value).toBe("Unsent context"); quiet();
});

it.each(["menu", "dialog"])("an external %s keeps Escape while the inspector is an overlay", async (kind) => {
  const { host, select, close, quiet } = setup(650); select(); await flush();
  const overlay = document.createElement(kind === "dialog" ? "dialog" : "div");
  if (kind === "dialog") overlay.setAttribute("open", ""); else overlay.setAttribute("role", "menu");
  document.body.append(overlay); escape(close());
  expect(host.querySelector("[data-team-detail]")).not.toBeNull(); overlay.remove();
  escape(close()); await flush(); expect(host.querySelector("[data-team-detail]")).toBeNull(); quiet();
});


it("Close returns to the current card when a same-team refresh replaced its original trigger", async () => {
  const { host, select, close, setView, quiet } = setup(650);
  const original = select(); await flush();
  setView(workspaceLayoutView()); await flush();
  expect(original.isConnected).toBe(false);
  const current = host.querySelector<HTMLButtonElement>('.team-task-card[data-node="1"]')!;
  close().click(); await flush();
  expect(document.activeElement).toBe(current); quiet();
});
