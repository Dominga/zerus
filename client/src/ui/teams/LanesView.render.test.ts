// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from "vitest";
import { createComponent, createSignal } from "solid-js";
import { render } from "solid-js/web";
import fixture from "./fixtures/lanes-view.json";
import type { TeamView } from "../../store/teams";
import { LanesView } from "./LanesView";
import { TeamWorkspace } from "./TeamTab";

// All positive members/cards/pages are pinned by the Rust operation writer.
const data = fixture as unknown as { before: TeamView; after: TeamView; pages: TeamView[]; closed: TeamView; empty: TeamView; ids: Record<string, number> };
let dispose: (() => void) | undefined;
afterEach(() => { dispose?.(); dispose = undefined; document.body.replaceChildren(); });
function show(view = data.before, workspace = false) {
  const [current, setView] = createSignal(view);
  const host = document.createElement("div"); document.body.append(host);
  const opened = vi.fn(), action = vi.fn(), page = vi.fn();
  dispose = render(() => workspace
    ? createComponent(TeamWorkspace, { get view() { return current(); }, busy: false, failure: null, onAction: action, onPage: page, onAddTask() {} })
    : createComponent(LanesView, { get view() { return current(); }, selected: null, onOpen: opened }), host);
  return { host, setView, opened, action, page };
}
const lane = (host: HTMLElement, member: string) => host.querySelector<HTMLElement>(`[data-member-lane="${member}"]`)!;
const button = (host: HTMLElement, label: string) => [...host.querySelectorAll<HTMLButtonElement>("button")].find((button) => button.textContent === label);

it("offers Lanes beside Board and Graph and opens the existing task detail without another read or action", () => {
  const { host, action, page } = show(data.before, true);
  expect(host.querySelector("[data-team-board]")).not.toBeNull();
  expect(button(host, "Lanes")).toBeDefined(); button(host, "Lanes")!.click();
  expect(host.querySelector("[data-team-lanes]")).not.toBeNull(); expect(host.querySelector("[data-team-board]")).toBeNull();
  expect(host.querySelector('[data-member="m3"] .team-member-activity')?.textContent ?? "").toContain("Reviewing #1");
  expect(host.querySelector('[data-member="m1"] .team-member-activity')?.textContent ?? "").toContain(`Working on #${data.ids.active}`);
  host.querySelector<HTMLButtonElement>('[data-lane-task="1"]')!.click();
  expect(host.querySelector("[data-team-detail] h3")?.textContent).toBe("Task 1");
  button(host, "Graph")!.click(); expect(host.querySelector("[data-team-graph]")).not.toBeNull();
  expect(host.querySelector("[data-team-detail] h3")?.textContent).toBe("Task 1");
  button(host, "Lanes")!.click(); expect(host.querySelector('[data-lane-task="1"]')?.getAttribute("aria-pressed")).toBe("true");
  expect(action).not.toHaveBeenCalled(); expect(page).not.toHaveBeenCalled();
});

it("puts the product-delivered review under its verified holder, apart from the task's implementer", () => {
  const { host } = show();
  expect(lane(host, "m3").querySelector('[data-lane-task="1"]')).not.toBeNull();
  expect(lane(host, "m1").querySelector('[data-lane-task="1"]')).toBeNull();
  expect(lane(host, "m3").querySelector('[data-lane-task="1"]')?.textContent ?? "").toContain("In progress");
  const orphan = data.pages.find((view) => view.groups.some((group) => group.cards.some((card) => card.kind === "stage")))!;
  dispose!(); dispose = undefined; host.remove();
  const next = show(orphan); const review = lane(next.host, "m3").querySelector<HTMLButtonElement>(`[data-lane-task="${data.ids.review}"]`);
  expect(review).not.toBeNull(); review!.click(); expect(next.opened).toHaveBeenCalledExactlyOnceWith(data.ids.review);
});

it("replaces one real page with another and uses the existing queue pager", () => {
  const { host, setView, page } = show(data.pages[0], true);
  button(host, "Lanes")!.click();
  expect(host.querySelector("[data-lanes-partial]")).not.toBeNull();
  expect(page).not.toHaveBeenCalled(); button(host, "More…")!.click(); expect(page).toHaveBeenCalledExactlyOnceWith(data.pages[0].next);
  for (const view of data.pages.slice(1)) {
    setView(view);
    expect(host.querySelector("[data-team-lanes]")).not.toBeNull();
    const expected = view.groups.flatMap((group) => group.cards.map((card) => card.node)).sort((a, b) => a - b);
    const actual = [...host.querySelectorAll("[data-lane-task]")].map((card) => Number(card.getAttribute("data-lane-task"))).sort((a, b) => a - b);
    expect(actual).toEqual(expected);
    if (view.groups.some((group) => group.cards.some((card) => card.kind === "stage"))) {
      expect(host.querySelector('[data-member="m3"] .team-member-activity')?.textContent ?? "").toContain("Reviewing #1");
    }
  }
});

it("keeps unchanged lane/card DOM and folds across an operation-written routing refresh", () => {
  const { host, setView } = show();
  const band = lane(host, "m1"), active = band.querySelector(`[data-lane-task="${data.ids.active}"]`);
  const header = lane(host, "m3").querySelector<HTMLButtonElement>(".team-lane-head")!;
  header.click(); expect(header.getAttribute("aria-expanded")).toBe("false");
  setView(data.after);
  expect(lane(host, "m1")).toBe(band); expect(band.querySelector(`[data-lane-task="${data.ids.active}"]`)).toBe(active);
  expect(lane(host, "m3").querySelector(".team-lane-head")).toBe(header); expect(header.getAttribute("aria-expanded")).toBe("false");
  header.click(); expect(lane(host, "m3").querySelector('[data-lane-task="1"]')).not.toBeNull();
});

it("prunes fold keys when the synthetic UI roster withholds and restores a member", () => {
  const { host, setView } = show();
  lane(host, "m3").querySelector<HTMLButtonElement>(".team-lane-head")!.click();
  // Negative scope metadata only; every team/card still comes from the product writer.
  setView({ ...data.before, members: data.before.members.filter((member) => member.id !== "m3") });
  expect(lane(host, "").querySelector('[data-lane-task="1"]')).not.toBeNull();
  setView(data.before);
  expect(lane(host, "m3").querySelector(".team-lane-head")?.getAttribute("aria-expanded")).toBe("true");
});

it("offers empty lanes for an empty queue without starting a timer", () => {
  const timer = vi.spyOn(globalThis, "setInterval");
  try {
    const { host, action, page } = show(data.empty, true); button(host, "Lanes")!.click();
    expect(host.querySelectorAll("[data-member-lane]")).toHaveLength(data.empty.members.length + 1);
    expect(host.querySelectorAll(".team-lane-empty")).toHaveLength(data.empty.members.length + 1);
    expect(action).not.toHaveBeenCalled(); expect(page).not.toHaveBeenCalled(); expect(timer).not.toHaveBeenCalled();
  } finally { timer.mockRestore(); }
});
