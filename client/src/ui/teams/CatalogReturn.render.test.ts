// @vitest-environment happy-dom
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { createComponent } from "solid-js";
import { render } from "solid-js/web";
import fixture from "../screens/fixtures/teams-screen.json";
import { catalog } from "./fixtures";
vi.mock("../../bridge", async (original) => ({ ...(await original<object>()), inTauri: () => true }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: async (command: string) => command === "teams_catalog" ? { ok: true, catalog } : undefined, isTauri: () => false }));
import { teams, type TeamView } from "../../store/teams";
import { page, setPage, setDetailTab } from "../../store/ui";
import { TeamsScreen } from "../screens/Teams";
import { TeamBar } from "./team-bar";
import { NewTeamDialog } from "./NewTeam";
import { CatalogPage } from "./CatalogPage";
const disposers: (() => void)[] = [];
afterEach(() => { for (const dispose of disposers.splice(0)) dispose(); teams.closeNewTeam(); document.body.replaceChildren(); });
beforeEach(async () => { await teams.recheck(); teams.apply([{ computer: "", key: "list", kind: "list", payload: fixture.running }]); teams.closeNewTeam(); });
function mount(component: () => import("solid-js").JSX.Element) {
  const host = document.createElement("div"); document.body.append(host); disposers.push(render(component, host)); return host;
}
function click(host: HTMLElement, text: string) {
  const button = [...host.querySelectorAll<HTMLButtonElement>("button")].find((button) => button.textContent === text);
  expect(button, text).toBeDefined(); button!.click();
}
it("returns Catalog Back to the actual Teams screen entry", () => {
  setPage("teams"); const entry = mount(() => createComponent(TeamsScreen, {})); click(entry, "Catalog");
  expect(page()).toBe("team-catalog"); const catalog = mount(() => createComponent(CatalogPage, {})); click(catalog, "Back");
  expect(page()).toBe("teams"); expect(teams.newTeamOpen()).toBe(false);
});
it("returns Catalog Back to the session Team tab that opened the actual team bar", () => {
  setPage("sessions"); setDetailTab("team");
  const entry = mount(() => createComponent(TeamBar, { view: fixture.team as unknown as TeamView, busy: false, onAction: vi.fn() })); click(entry, "Catalog");
  const catalog = mount(() => createComponent(CatalogPage, {})); click(catalog, "Back");
  expect(page()).toBe("sessions"); expect(teams.newTeamOpen()).toBe(false);
});
it("returns Catalog Back to the New team form on its original page", async () => {
  setPage("projects"); await teams.openNewTeam(); const entry = mount(() => createComponent(NewTeamDialog, {})); click(entry, "Catalog");
  expect(page()).toBe("team-catalog"); expect(teams.newTeamOpen()).toBe(false);
  const catalog = mount(() => createComponent(CatalogPage, {})); click(catalog, "Back");
  expect(page()).toBe("projects"); expect(teams.newTeamOpen()).toBe(true);
});
