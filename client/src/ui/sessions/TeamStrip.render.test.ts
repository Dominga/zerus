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
afterEach(() => { dispose?.(); document.body.innerHTML = ""; });
function mount(team = data.running.teams[0]) {
  const host = document.createElement("div"); document.body.append(host);
  const toggle = vi.fn(), outer = vi.fn();
  dispose = render(() => createComponent(Dynamic, { component: "div", onClick: outer,
    get children() { return createComponent(TeamStrip, { team, expanded: true, onToggle: toggle }); } }), host);
  return { host, toggle, outer };
}

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
  it("folds with either the strip or its keyboard button and never selects the lead as a side effect", () => {
    const { host, toggle, outer } = mount();
    host.querySelector<HTMLElement>("[data-team-strip]")!.click();
    host.querySelector<HTMLButtonElement>("[data-team-toggle]")!.click();
    expect(toggle).toHaveBeenCalledTimes(2); expect(outer).not.toHaveBeenCalled();
    expect(host.querySelector("button")?.getAttribute("aria-expanded")).toBe("true");
  });
  it("shows future multi-phase progress and capped counts honestly", () => {
    // Explicit UI controls: no product writer currently creates a second phase
    // or a thousand-member team. Positive list data above is product-generated.
    const base = data.running.teams[0];
    const team = { ...base, progress: { ...base.progress!, phases: 3, phases_done: 2, tasks: 1000, tasks_more: true } };
    const { host } = mount(team);
    expect([...host.querySelectorAll(".tbar-label")].map((n) => n.textContent)).toEqual(["1/1000+", "2/3"]);
    expect(host.querySelector<HTMLElement>(".tbar-fill")?.style.width).toBe("0%");
  });
});
