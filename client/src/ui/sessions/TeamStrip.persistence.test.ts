// @vitest-environment happy-dom
import { describe, expect, it, vi } from "vitest";
import { createComponent } from "solid-js";
import { render } from "solid-js/web";
import fixture from "./fixtures/team-progress.json";
import presentation from "./fixtures/team-session-presentation.json";
import type { TeamList } from "../../store/teams";
import type { ViewRow } from "../../types";
const backing = vi.hoisted(() => ({ settings: {} as Record<string, unknown> }));
vi.hoisted(() => vi.stubGlobal("location", new URL("http://localhost/")));
vi.mock("../../store/settings", async (original) => ({ ...await original<typeof import("../../store/settings")>(),
  settings: backing.settings, settingsReady: Promise.resolve(),
  setSetting: vi.fn(async (key: string, value: unknown) => { backing.settings[key] = value; return true; }) }));

describe("the ordinary children control owns the durable team fold", () => {
  it("saves through the children chip and restores in fresh stores for both card sizes", async () => {
    const team = (fixture as unknown as { running: TeamList }).running.teams[0];
    const lead = team.lead_session!, sessionId = lead.session_id.slice("launch_id:".length), key = "\nrun:lead";
    const mount = async (compact: boolean) => {
      const store = await import("../../store/teamCollapse");
      const { applyBatch, toSession } = await import("../../store/catalog");
      const { applyMachines } = await import("../../store/machines");
      const { emptyBatch } = await import("../../types");
      const { SessionCard, CompactCard } = await import("./SessionCard");
      const raw = { ...presentation.ready, session_id: sessionId } as unknown as ViewRow;
      applyBatch({ ...emptyBatch(), upserts: [{ computer: "", key: "run:lead", row: raw }] });
      applyMachines({ ...emptyBatch(), machines: [{ computer: "", machine: { machine_id: lead.machine } as never }] });
      const host = document.createElement("div"); document.body.append(host);
      const onOpenTeam = vi.fn();
      const shared = { session: toSession("", "run:lead", raw), selected: false, agentTeamSummary: team, onOpenTeam,
        get team() { return { total: 3, working: 0, attention: 0, unread: 0, collapsed: store.teamCollapsed(key) }; },
        onToggleTeam: () => store.setTeamCollapsed(key, !store.teamCollapsed(key)) };
      const dispose = render(() => compact ? createComponent(CompactCard, { ...shared, get team() { return shared.team; }, onExpand() {} })
        : createComponent(SessionCard, { ...shared, get team() { return shared.team; }, expandable: false, expanded: false, onToggleChildren() {} }), host);
      return { store, host, onOpenTeam, dispose: () => { dispose(); host.remove(); } };
    };
    let view = await mount(false);
    view.host.querySelector<HTMLButtonElement>("[data-team-strip]")!.click();
    expect(view.onOpenTeam).toHaveBeenCalledOnce();
    expect(view.store.teamCollapsed(key)).toBe(false);
    expect(backing.settings[view.store.COLLAPSED_TEAMS]).toBeUndefined();
    view.host.querySelector<HTMLButtonElement>("button[aria-expanded]")!.click();
    await Promise.resolve(); await Promise.resolve();
    expect(backing.settings[view.store.COLLAPSED_TEAMS]).toBe(JSON.stringify([`${lead.machine}\n${sessionId}`]));
    view.dispose(); vi.resetModules();
    view = await mount(true);
    expect(view.store.teamCollapsed(key)).toBe(true);
    expect(view.host.querySelector("button[aria-expanded]")?.getAttribute("aria-expanded")).toBe("false");
    expect(view.host.querySelector("[data-team-strip] [aria-expanded]")).toBeNull();
    view.host.querySelector<HTMLButtonElement>("button[aria-expanded]")!.click();
    await Promise.resolve(); await Promise.resolve();
    expect(backing.settings[view.store.COLLAPSED_TEAMS]).toBe("[]");
    view.dispose();
  });
});
