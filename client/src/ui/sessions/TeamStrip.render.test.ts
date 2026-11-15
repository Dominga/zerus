// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from "vitest";
import { createComponent } from "solid-js";
import { Dynamic, render } from "solid-js/web";
import fixture from "./fixtures/team-progress.json";
import type { TeamList } from "../../store/teams";
import { TeamStrip } from "./TeamStrip";
vi.hoisted(() => vi.stubGlobal("location", new URL("http://localhost/")));
const data = fixture as unknown as { running: TeamList; stuck: TeamList };
let dispose: (() => void) | undefined;
afterEach(() => { dispose?.(); document.body.innerHTML = ""; locale.language = "en"; });
function mount(team = data.running.teams[0]) {
  const host = document.createElement("div"); document.body.append(host);
  const open = vi.fn(), outer = vi.fn();
  dispose = render(() => createComponent(Dynamic, { component: "div", onClick: outer,
    get children() { return createComponent(TeamStrip, { team, onOpen: open }); } }), host);
  return { host, open, outer };
}

const locale = vi.hoisted(() => ({ language: "en" as "en" | "ru" }));
vi.mock("../../i18n", async (original) => {
  const actual = await original<typeof import("../../i18n")>();
  const ru = (await import("../../i18n.ru.json")).default;
  return { ...actual, tr: (source: string, ...args: (string | number)[]) => actual.translateWithCatalog(ru, locale.language, source, ...args) };
});

describe("lead strip from the real CLI/core list payload", () => {
  it("shows accepted/total tasks, questions, and live members plus the lead, without a phase bar for the queue", () => {
    const { host } = mount();
    expect(host.querySelectorAll(".tbar")).toHaveLength(1);
    expect(host.querySelector(".tbar-label")?.textContent).toBe("1/3");
    expect(host.querySelector<HTMLElement>(".tbar-fill")?.style.width).toBe("33%");
    expect(host.querySelector(".sc-team-status")?.textContent).toBe("Running");
    expect(host.querySelector(".sc-team-answers")?.textContent).toBe("?1");
    expect(host.querySelector(".sc-team-count")?.textContent).toBe("4 members");
    expect(host.querySelector(".stuck")).toBeNull();
  });
  it("highlights an actual blocking card without changing the team status", () => {
    const team = data.stuck.teams[0];
    expect(team.status).toBe("running");
    expect(mount(team).host.querySelector(".sc-team-status.stuck")?.textContent).toBe("Stuck");
  });
  it("is one labelled button: click, Enter and Space open without bubbling or a fold chevron", () => {
    const { host, open, outer } = mount();
    const button = host.querySelector<HTMLButtonElement>("button[data-team-strip]")!;
    expect(button.type).toBe("button");
    expect(button.getAttribute("aria-label")).toBe("Open the team Fixes");
    button.click();
    for (const key of ["Enter", " "]) {
      const e = new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true });
      button.dispatchEvent(e);
      expect(e.defaultPrevented).toBe(true);
    }
    expect(open).toHaveBeenCalledTimes(3);
    expect(outer).not.toHaveBeenCalled();
    expect(host.querySelector("[data-team-toggle], .chev, [aria-expanded]")).toBeNull();
    expect(host.querySelectorAll("button")).toHaveLength(1);
  });
  it("shows future multi-phase progress and capped counts honestly", () => {
    // Explicit UI controls: no product writer currently creates a second phase
    // or a thousand-member team. Positive list data above is product-generated.
    const base = data.running.teams[0];
    const team = { ...base, progress: { ...base.progress!, phases: 3, phases_done: 2, tasks: 1000, accepted: 500, tasks_more: true } };
    const { host } = mount(team);
    expect([...host.querySelectorAll(".tbar-label")].map((n) => n.textContent)).toEqual(["500/1000+", "2/3"]);
    expect(host.querySelector<HTMLElement>(".tbar-fill")?.style.width).toBe("0%");
  });
});

it("omits zero questions", () => {
  const base = data.running.teams[0];
  expect(mount({ ...base, progress: { ...base.progress!, questions: 0 } }).host.querySelector(".sc-team-answers")).toBeNull();
});
it.each([
  ["en", 1, "1 member"], ["en", 2, "2 members"],
  ["ru", 1, "1 участник"], ["ru", 2, "2 участника"], ["ru", 5, "5 участников"],
  ["ru", 11, "11 участников"], ["ru", 21, "21 участник"],
] as const)("uses %s plurals for %s members", (language, members, label) => {
  locale.language = language;
  const base = data.running.teams[0];
  expect(mount({ ...base, progress: { ...base.progress!, members } }).host.querySelector(".sc-team-count")?.textContent).toBe(label);
});

it("keeps all capped labels and suppresses percentages for either lower-bound flag", () => {
  // Explicit rendering controls for independently capped fields and future phases.
  const base = data.running.teams[0];
  const { host } = mount({ ...base, progress: { ...base.progress!, accepted: 500, accepted_more: true,
    tasks: 900, tasks_more: false, members: 1000, members_more: true, questions: 1000, questions_more: true,
    phases: 1000, phases_more: true, phases_done: 500 } });
  expect([...host.querySelectorAll(".tbar-label")].map((n) => n.textContent)).toEqual(["500+/900", "500/1000+"]);
  expect([...host.querySelectorAll<HTMLElement>(".tbar-fill")].map((n) => n.style.width)).toEqual(["0%", "0%"]);
  expect(host.querySelector(".sc-team-answers")?.textContent).toBe("?1000+");
  expect(host.querySelector(".sc-team-count")?.textContent).toBe("1000+ members");
});
