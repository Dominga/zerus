// @vitest-environment happy-dom
// The team bar (the fork's strip, adapted to M1): the name, the status, the
// progress track and count from the core's counts, the chips and the team's
// own operations.
import { afterEach, expect, it, vi } from "vitest";
import { createComponent } from "solid-js";
import { render } from "solid-js/web";
import { TeamBar } from "./team-bar";
import { teamView } from "./fixtures";
import { translateWithCatalog } from "../../i18n";
import russian from "../../i18n.ru.json";

let dispose: (() => void) | undefined;
afterEach(() => { dispose?.(); dispose = undefined; document.body.replaceChildren(); });

const setup = (view = teamView, onAction = vi.fn(), width = 1300) => {
  const host = document.createElement("div");
  document.body.append(host);
  dispose = render(() => createComponent(TeamBar, { view, busy: false, onAction, width }), host);
  return { host, onAction };
};

it("shows the team's name, status and progress", () => {
  const { host } = setup();
  expect(host.querySelector(".team-name")?.textContent).toBe("Fixes");
  expect(host.querySelector(".tm-state-badge")?.textContent).toBe("Running");
  expect(host.querySelector(".tm-state-badge")?.getAttribute("data-state")).toBe("running");
  expect(host.querySelector(".team-count")?.textContent).toBe("0/2");
  // One segment for each non-zero count: one reported and one waiting.
  expect(host.querySelectorAll(".team-progress .seg").length).toBe(2);
});

it("shows the counts the core gave, a singular for one and a plural for more", () => {
  const chips = (inbox: number, cards: number) => {
    const { host } = setup({ ...teamView, bar: { ...teamView.bar, counts: { ...teamView.bar.counts, inbox, cards } } });
    const text = [...host.querySelectorAll("[data-strip-chips] .team-chip")].map((chip) => chip.textContent);
    dispose?.(); dispose = undefined; host.remove();
    return text;
  };
  expect(chips(1, 1)).toEqual(["2 active", "1 inbox item", "1 card"]);
  expect(chips(2, 3)).toEqual(["2 active", "2 inbox items", "3 cards"]);
  expect(chips(0, 0)).toEqual(["2 active", "0 inbox items", "0 cards"]);
});

it("words the counts with the catalog's three Russian plural forms", () => {
  const forms = (n: number) => [n === 1 ? "%n card" : "%n cards", n === 1 ? "%n inbox item" : "%n inbox items"].map((source) => translateWithCatalog(russian, "ru", source, n));
  expect(forms(1)).toEqual(["1 карточка", "1 входящий"]);
  expect(forms(2)).toEqual(["2 карточки", "2 входящих"]);
  expect(forms(5)).toEqual(["5 карточек", "5 входящих"]);
  expect(forms(21)).toEqual(["21 карточка", "21 входящий"]);
});

it("offers the team's own operations by its status", () => {
  const { host, onAction } = setup();
  const buttons = [...host.querySelectorAll("[data-strip-actions] button")].map((button) => button.getAttribute("data-strip-action"));
  expect(buttons).toEqual(["pause", "stop"]);
  (host.querySelector('[data-strip-action="stop"]') as HTMLButtonElement).click();
  expect(onAction).toHaveBeenCalledWith("stop");
});

it("folds the action row by the strip's own width while keeping operations reachable", () => {
  expect(setup(teamView, vi.fn(), 1300).host.querySelector("[data-strip-actions]")).not.toBeNull();
  for (const width of [1000, 850, 320]) {
    const { host, onAction } = setup(teamView, vi.fn(), width);
    expect(host.querySelector(".team-bar")?.getAttribute("data-compact")).toBe(width === 1000 ? "low" : "high");
    expect([...host.querySelectorAll("[data-strip-action]")].map((button) => button.getAttribute("data-strip-action"))).toEqual(["pause", "stop"]);
    (host.querySelector('[data-strip-action="pause"]') as HTMLButtonElement).click();
    expect(onAction).toHaveBeenCalledWith("pause");
  }
});

it("offers resume to a paused team and archive to a stopped one", () => {
  const paused = { ...teamView, bar: { ...teamView.bar, status: "paused" } };
  expect([...setup(paused).host.querySelectorAll("[data-strip-actions] button")].map((button) => button.getAttribute("data-strip-action"))).toEqual(["resume", "stop"]);
  const stopped = { ...teamView, bar: { ...teamView.bar, status: "stopped" } };
  expect([...setup(stopped).host.querySelectorAll("[data-strip-actions] button")].map((button) => button.getAttribute("data-strip-action"))).toEqual(["archive"]);
});
