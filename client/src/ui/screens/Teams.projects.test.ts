// @vitest-environment happy-dom
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { createComponent } from "solid-js";
import { render } from "solid-js/web";
import fixture from "./fixtures/teams-projects.json";
import type { ProjectsView } from "../../types";
import type { TeamList } from "../../store/teams";

// Both the list and ProjectsView are pinned by the Rust product-writer test.
const data = fixture as unknown as { list: TeamList; projects: ProjectsView; renamed_projects: ProjectsView };
const f = vi.hoisted(() => ({
  reply: undefined as ProjectsView | undefined,
  changed: (_screen: "projects") => {}, pull: vi.fn(), visible: vi.fn(),
}));
vi.mock("../../bridge", async (original) => ({
  ...(await original<object>()),
  projectsView: async () => { f.pull(); return f.reply!; },
  screenVisible: async (...args: unknown[]) => { f.visible(...args); },
  onScreensChanged: (changed: typeof f.changed) => { f.changed = changed; return () => {}; },
}));
let ui: typeof import("../../store/ui");
let teams: typeof import("../../store/teams").teams;
let TeamsScreen: typeof import("./Teams").TeamsScreen;
let startScreens: typeof import("../../store/screens").startScreens;
let dispose: (() => void) | undefined;
let stop: (() => void) | undefined;
beforeEach(async () => {
  vi.resetModules(); f.reply = data.projects; f.pull.mockClear(); f.visible.mockClear();
  ui = await import("../../store/ui"); ({ teams } = await import("../../store/teams"));
  ({ startScreens } = await import("../../store/screens")); ({ TeamsScreen } = await import("./Teams"));
  ui.setPage("teams");
});
afterEach(() => { stop?.(); stop = undefined; dispose?.(); dispose = undefined; document.body.replaceChildren(); });

it("shows the product project name when Teams is the first screen and refreshes through the same hub", async () => {
  teams.apply([{ computer: "", key: "list", kind: "list", payload: data.list }]);
  const host = document.createElement("div"); document.body.append(host);
  stop = startScreens(ui.page, { hidden: () => false, onChange: () => () => {} });
  dispose = render(() => createComponent(TeamsScreen, {}), host);
  await vi.waitFor(() => expect(host.querySelector("[data-team-row]")?.textContent).toContain("Example project"));
  expect(host.querySelector("[data-team-row]")?.textContent).not.toContain(data.list.teams[0].project);
  expect(f.pull).toHaveBeenCalledTimes(1); expect(f.visible.mock.calls).toEqual([["projects", true]]);
  f.reply = data.renamed_projects; f.changed("projects");
  await vi.waitFor(() => expect(host.querySelector("[data-team-row]")?.textContent).toContain("Renamed project"));
  expect(f.pull).toHaveBeenCalledTimes(2);
  ui.setPage("projects"); await Promise.resolve();
  expect(f.visible.mock.calls).toEqual([["projects", true]]); expect(f.pull).toHaveBeenCalledTimes(2);
});
