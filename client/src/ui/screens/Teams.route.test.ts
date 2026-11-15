// @vitest-environment happy-dom
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { createComponent } from "solid-js";
import { render } from "solid-js/web";
import fixture from "./fixtures/teams-screen.json";
import { catalog } from "../teams/fixtures";

// Static imports preload the real App and both lazy destinations during
// collection, outside the default 5s interaction budget. This file keeps one
// module graph, so preloading cannot leave old stores alive across resetModules.
vi.mock("../../bridge", async (original) => ({ ...(await original<object>()), inTauri: () => true }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: async (command: string) => command === "teams_catalog" ? { ok: true, catalog } : undefined, isTauri: () => false }));
import { App } from "../../App";
import "./Teams";
import "./Waiting";
import "../teams/NewTeam";
import { teams } from "../../store/teams";
import { page, setPage, setSelectedKey, setDetailTab } from "../../store/ui";
import { settingsReady, setSetting } from "../../store/settings";
let dispose: (() => void) | undefined;
beforeEach(async () => {
  await settingsReady; await setSetting("shortcuts/team.new", null, { persist: false });
  await teams.recheck(); teams.apply([{ computer: "", key: "list", kind: "list", payload: fixture.running }]);
  teams.closeNewTeam(); setSelectedKey(undefined); setPage("teams"); setDetailTab("activity");
});
afterEach(() => { dispose?.(); dispose = undefined; document.body.replaceChildren(); });
function show() {
  const host = document.createElement("div"); document.body.append(host);
  dispose = render(() => createComponent(App, { mode: () => "replay" }), host); return host;
}

it("mounts the actual lazy Teams route from the application's rail", async () => {
  const host = show();
  await vi.waitFor(() => expect(host.querySelector("[data-teams-page]")).not.toBeNull());
  expect(host.querySelector("[data-team-row]")?.textContent).toContain("Fixes");
  const rail = host.querySelector('button[aria-label="Teams"]') as HTMLButtonElement;
  expect(rail.getAttribute("aria-current")).toBe("page");
  (host.querySelector('button[aria-label="Waiting for you"]') as HTMLButtonElement).click(); expect(page()).toBe("waiting");
  rail.click(); expect(page()).toBe("teams");
  await vi.waitFor(() => expect(host.querySelector("[data-teams-page]")).not.toBeNull());
});

it("opens New team through the configured team.new keyboard command and respects preview off", async () => {
  show(); await setSetting("shortcuts/team.new", ["Ctrl+Alt+N"], { persist: false });
  const press = () => document.body.dispatchEvent(new KeyboardEvent("keydown", { key: "n", code: "KeyN", ctrlKey: true, altKey: true, bubbles: true, cancelable: true }));
  press(); expect(teams.newTeamOpen()).toBe(true);
  teams.closeNewTeam(); teams.apply([{ computer: "", key: "list", kind: "failed", payload: { code: "teams_inactive", message: "off" } }]);
  press(); expect(teams.newTeamOpen()).toBe(false);
});
