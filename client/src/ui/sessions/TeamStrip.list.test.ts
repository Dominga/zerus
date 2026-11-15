// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from "vitest";
import { createComponent } from "solid-js";
import { render } from "solid-js/web";
import fixture from "./fixtures/agent-team-views.json";
import presentation from "./fixtures/team-session-presentation.json";
import type { TeamList, TeamView } from "../../store/teams";
import type { ViewRow } from "../../types";
vi.hoisted(() => vi.stubGlobal("location", new URL("http://localhost/")));
vi.mock("../../bridge", async (original) => ({ ...await original<object>(), gitRows: vi.fn().mockResolvedValue(undefined) }));
vi.mock("@tanstack/solid-virtual", () => ({
  createVirtualizer: (options: { count: number; getItemKey(n: number): string }) => ({
    measure() {}, _willUpdate() {}, scrollToIndex() {}, getTotalSize: () => options.count * 122,
    getVirtualItems: () => Array.from({ length: options.count }, (_, index) => ({ key: options.getItemKey(index), start: index * 122, index })),
  }),
}));
import { SessionList } from "./SessionList";
import { applyBatch } from "../../store/catalog";
import { applyMachines } from "../../store/machines";
import { teams } from "../../store/teams";
import { emptyBatch } from "../../types";
import { detailTab, selectedKey, setDetailTab, setSelectedKey, setStripMode, setCompactRows } from "../../store/ui";
let dispose: (() => void) | undefined;
afterEach(() => { dispose?.(); document.body.replaceChildren(); vi.unstubAllGlobals(); });

it.each([[false, "click"], [true, "click"], [false, "Enter"], [true, " "]] as const)("the actual list wires its compact=%s strip via %s to that session's Team tab", async (compact, activation) => {
  const { list, team } = fixture as unknown as { list: TeamList; team: TeamView };
  const leadKey = "\nrun:lead";
  applyMachines({ ...emptyBatch(), machines: [{ computer: "", machine: { machine_id: list.teams[0].home } as never }] });
  applyBatch({ ...emptyBatch(), computers: [{ computer: "", status: "live", attempt: 1 }], replaces: [{ computer: "", rows: team.members.filter((m) => m.session).map((m) => ({
    computer: "", key: `run:${m.id}`, row: {
      ...(m.lead ? presentation.ready : presentation.unknown), name: `codex/unrelated/${m.id}`, title: m.id,
      launch_id: m.session!.session_id.slice("launch_id:".length), session_id: m.session!.session_id.slice("launch_id:".length),
      run_id: m.id, conversation_id: m.lead ? "lead-conversation" : "",
    } as unknown as ViewRow,
  })) }] });
  teams.apply([{ computer: "", key: "list", kind: "list", payload: list }]);
  setStripMode(false); setCompactRows(leadKey, compact);
  setSelectedKey("another-session"); setDetailTab("terminal");
  const host = document.createElement("div"); document.body.append(host);
  dispose = render(() => createComponent(SessionList, {}), host);
  for (let n = 0; n < 12; n++) await Promise.resolve();
  expect(host.querySelectorAll("[data-team-strip]")).toHaveLength(1);
  const strip = host.querySelector<HTMLButtonElement>("[data-team-strip]")!;
  if (activation === "click") strip.click();
  else strip.dispatchEvent(new KeyboardEvent("keydown", { key: activation, bubbles: true, cancelable: true }));
  expect(selectedKey()).toBe(leadKey);
  expect(detailTab()).toBe("team");
});
