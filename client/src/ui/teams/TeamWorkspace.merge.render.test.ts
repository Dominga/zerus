// @vitest-environment happy-dom
// Graph and feed rows are the product writers' pinned core projections.
// Only the read envelope is rebound to the graph fixture's team and cursor.
import { afterEach, expect, it, vi } from "vitest";
import { createComponent } from "solid-js";
import { render } from "solid-js/web";
import { TeamWorkspace } from "./TeamTab";
import graph from "../../../core/src/teams/fixtures/graph-view.json";
import pages from "../../../core/src/teams/fixtures/drawers.json";
import type { TeamView } from "./types";
import type { DrawerRead } from "../../store/teamDrawers";

vi.mock("../../store/teamDrawers", async (original) => {
  const actual = await original<typeof import("../../store/teamDrawers")>();
  const { default: data } = await import("../../../core/src/teams/fixtures/drawers.json");
  return { ...actual, readTeamDrawer: async (read: DrawerRead) => ({ ...data.feed, team: read.team, after: read.after }) };
});

let dispose: (() => void) | undefined;
afterEach(() => { dispose?.(); document.body.replaceChildren(); });

it("keeps Graph and Board beside the drawer controls without losing the graph when a drawer opens", async () => {
  const host = document.createElement("div"); document.body.append(host);
  dispose = render(() => createComponent(TeamWorkspace, {
    view: graph as unknown as TeamView, busy: false, failure: null,
    onAction: vi.fn(), onPage: vi.fn(), onAddTask: vi.fn(),
  }), host);
  expect(host.querySelectorAll("[data-team-drawer]")).toHaveLength(5);
  expect(host.querySelector("[data-team-board]")).not.toBeNull();
  host.querySelector<HTMLButtonElement>("[data-team-graph-mode]")!.click();
  const canvas = host.querySelector("[data-team-graph]");
  expect(canvas?.querySelectorAll("[data-plan-task]").length).toBeGreaterThan(0);
  host.querySelector<HTMLButtonElement>('[data-team-drawer="feed"]')!.click();
  for (let n = 0; n < 8; n++) await Promise.resolve();
  const drawer = host.querySelector('[data-team-drawer-panel="feed"]');
  expect(drawer?.textContent).toContain(pages.feed.rows[0].text);
  expect(host.querySelector("[data-team-graph]")).toBe(canvas);
  drawer!.querySelector<HTMLButtonElement>('[aria-label="Close"]')!.click();
  expect(host.querySelector("[data-team-drawer-panel]")).toBeNull();
  expect(host.querySelector("[data-team-graph]")).toBe(canvas);
  host.querySelector<HTMLButtonElement>(".team-board-head button")!.click();
  expect(host.querySelector("[data-team-board]")).not.toBeNull();
  expect(host.querySelectorAll("[data-team-drawer]")).toHaveLength(5);
});
