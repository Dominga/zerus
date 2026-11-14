// @vitest-environment happy-dom
import { describe, expect, it, vi } from "vitest";
import { createComponent } from "solid-js";
import { render } from "solid-js/web";
import fixture from "./fixtures/team-progress.json";
import type { TeamList } from "../../store/teams";
import { TeamStrip } from "./TeamStrip";
const backing = vi.hoisted(() => ({ settings: {} as Record<string, unknown>, rows: {} as Record<string, unknown>, machines: {} as Record<string, unknown> }));
vi.hoisted(() => vi.stubGlobal("location", new URL("http://localhost/")));
vi.mock("../../store/settings", () => ({ settings: backing.settings, settingsReady: Promise.resolve(),
  setSetting: vi.fn(async (key: string, value: unknown) => { backing.settings[key] = value; return true; }) }));
vi.mock("../../store/catalog", () => ({ catalog: { rows: backing.rows } }));
vi.mock("../../store/machines", () => ({ machineCards: backing.machines }));

describe("team strip uses the existing durable parent fold", () => {
  it("restores the fold in a fresh store after the strip saves it", async () => {
    const team = (fixture as unknown as { running: TeamList }).running.teams[0];
    const lead = team.lead_session!, session = lead.session_id.slice("launch_id:".length), key = "\nrun:lead";
    backing.rows[key] = { computer: "", sessionId: session };
    backing.machines[""] = { machine_id: lead.machine };
    let store = await import("../../store/teamCollapse");
    const host = document.createElement("div"); document.body.append(host);
    let dispose = render(() => createComponent(TeamStrip, { team, get expanded() { return !store.teamCollapsed(key); }, onToggle: () => store.setTeamCollapsed(key, !store.teamCollapsed(key)) }), host);
    host.querySelector<HTMLButtonElement>("button")!.click();
    await Promise.resolve(); await Promise.resolve();
    expect(backing.settings[store.COLLAPSED_TEAMS]).toBe(JSON.stringify([`${lead.machine}\n${session}`]));
    dispose(); vi.resetModules();
    store = await import("../../store/teamCollapse");
    expect(store.teamCollapsed(key)).toBe(true);
    dispose = render(() => createComponent(TeamStrip, { team, get expanded() { return !store.teamCollapsed(key); }, onToggle: () => store.setTeamCollapsed(key, !store.teamCollapsed(key)) }), host);
    expect(host.querySelector("button")?.getAttribute("aria-expanded")).toBe("false");
    host.querySelector<HTMLElement>("[data-team-strip]")!.click();
    await Promise.resolve(); await Promise.resolve();
    expect(backing.settings[store.COLLAPSED_TEAMS]).toBe("[]");
    dispose(); host.remove();
  });
});
