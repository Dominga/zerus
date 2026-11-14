// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createComponent } from "solid-js";
import { render } from "solid-js/web";
import type { Session } from "./types";
import type { GroupingOut, ViewRow } from "../../types";
import teamFixture from "./fixtures/agent-team-views.json";

const fixture = vi.hoisted(() => ({
  scroll: vi.fn(), index: (): number => 0, move: (_n: number) => {}, saved: undefined as Session | undefined,
  media: undefined as ((event: { matches: boolean }) => void) | undefined,
  inspect: vi.fn(async () => null), archive: vi.fn(async () => ({ rows: [], next_cursor: "", previous_cursor: null, total: 0, position: 0 })),
  collapse: (_id: string, _collapsed: boolean) => {},
}));
vi.hoisted(() => {
  vi.stubGlobal("matchMedia", (query: string) => ({ matches: false, media: query,
    addEventListener: (_name: string, run: (event: { matches: boolean }) => void) => { fixture.media = run; },
    removeEventListener() {},
  }));
});
vi.mock("../../bridge", async (original) => ({ ...await original<object>(),
  inspectSession: fixture.inspect, archivePage: fixture.archive, gitRows: vi.fn(async () => undefined),
  windowVisibility: { hidden: () => false, onChange: () => () => {} },
  setProjectCollapsed: vi.fn(async (id: string, collapsed: boolean) => { fixture.collapse(id, collapsed); }),
}));
vi.mock("./SearchResults", () => ({ SearchResults: () => null }));
vi.mock("@tanstack/solid-virtual", async () => {
  const { createSignal } = await import("solid-js");
  const [index, setIndex] = createSignal(0); fixture.index = index; fixture.move = setIndex;
  return { createVirtualizer: (options: { count: number; getItemKey(n: number): string }) => ({
    measure() {}, measureElement() {}, _willUpdate() {}, getTotalSize: () => options.count * 94,
    scrollToIndex: (n: number, alignment: unknown) => { fixture.scroll(n, alignment); setIndex(n); },
    getVirtualItems: () => Array.from({ length: Math.min(6, Math.max(0, options.count - index())) }, (_, offset) => {
      const n = index() + offset; return { key: options.getItemKey(n), start: n * 94, index: n };
    }),
  }) };
});

import { ShowInListButton } from "./ShowInList";
import { SessionList } from "./SessionList";
import { listRows } from "./model";
import { applyBatch, toSession } from "../../store/catalog";
import { applyGrouping, grouping } from "../../store/organization";
import { applyMachines } from "../../store/machines";
import { teams } from "../../store/teams";
import { setTeamCollapsed, teamCollapsed } from "../../store/teamCollapse";
import { settingsReady } from "../../store/settings";
import { emptyBatch } from "../../types";
import { forgetListReveal, listRevealPulse } from "../../store/listReveal";
import { bindSearchRow, machineFilter, projectFilter, accountFilter, folderFilter, search, selected, selectedKey,
  setPage, setSelectedKey, setSearch, setStateFilter, setMachineFilter, setProjectFilter, setAccountFilter, setFolderFilter,
  setStripMode, stripMode, setPeek, setSubagentOpen, subagentOpen, stateFilter } from "../../store/ui";

const MACHINE = "00000000-0000-4000-8000-0000000000aa";
const uuid = (n: number) => `00000000-0000-4000-8000-${String(n).padStart(12, "0")}`;
const key = (n: number, computer = "") => `${computer}\nrun:locate-${n}`;
const row = (n: number, extra: Partial<ViewRow> = {}): ViewRow => ({ name: `codex/locate/session-${n}`, title: `Session ${n}`,
  agent: "codex", status: "ready", state: "running", run_id: `locate-${n}`, conversation_id: `conversation-${n}`, session_id: uuid(n),
  cwd: `/fixture/${n}`, ...extra });
const header = (id: string, collapsed: boolean) => ({ id, name: id, color: null, accessible: true, collapsed, is_default: id === "alpha", palette: 0 });
function put(input: ViewRow[], computer = "", groups = input.map(() => "alpha"), collapsed = false) {
  applyMachines({ ...emptyBatch(), machines: [{ computer, machine: { machine_id: MACHINE } as never }] });
  applyBatch({ ...emptyBatch(), computers: input.length ? [{ computer, status: "live", attempt: 0 }] : [],
    replaces: [{ computer, rows: input.map((v) => ({ computer, key: `run:${v.run_id}`, row: v })) }] });
  const grouping: GroupingOut = { loaded: true, default_project: "alpha", ready: true, reason: null, busy: false, status: "", status_error: false, conflicts: 0,
    headers: [header("alpha", collapsed), header("beta", collapsed)] };
  applyGrouping({ ...emptyBatch(), grouping, groups: input.map((v, i) => ({ computer, key: `run:${v.run_id}`, group: groups[i], rank: i })) });
}
let host: HTMLElement, dispose: (() => void) | undefined;
const flush = async () => { for (let n = 0; n < 30; n++) await Promise.resolve(); };
function mount() {
  host = document.createElement("div"); document.body.append(host);
  dispose = render(() => [createComponent(ShowInListButton, {}), createComponent(SessionList, {})], host);
}
const locate = async () => { host.querySelector<HTMLButtonElement>('[aria-label="Show in session list"]')!.click(); await flush(); };
const shownRow = (id: string) => [...host.querySelectorAll<HTMLElement>(".vrow")].find((el) => el.dataset.key === id);

beforeEach(async () => {
  await settingsReady;
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
  vi.stubGlobal("ResizeObserver", class { observe() {} disconnect() {} });
  vi.stubGlobal("requestAnimationFrame", (run: FrameRequestCallback) => { queueMicrotask(() => run(performance.now())); return 1; });
  vi.stubGlobal("cancelAnimationFrame", () => {});
  fixture.media?.({ matches: false }); fixture.move(0); fixture.saved = undefined;
  fixture.collapse = (id, collapsed) => applyGrouping({ ...emptyBatch(), grouping: { ...grouping(), headers: grouping().headers.map((h) => h.id === id ? { ...h, collapsed } : h) } });
  forgetListReveal(); setSelectedKey(undefined); setPage("sessions"); setSearch(""); setStateFilter("all");
  setMachineFilter([]); setProjectFilter([]); setAccountFilter([]); setFolderFilter(undefined); setSubagentOpen(undefined); setStripMode(false); setPeek(false);
  bindSearchRow((id) => fixture.saved?.key === id ? fixture.saved : undefined);
  teams.apply([{ computer: "", key: "list", kind: "list", payload: { teams: [], truncated: false, next: null } }]);
  applyBatch({ ...emptyBatch(), computers: ["", "peer"].map((computer) => ({ computer, status: "ended", attempt: 0 })),
    replaces: [{ computer: "", rows: [] }, { computer: "peer", rows: [] }] });
  vi.clearAllMocks();
});
afterEach(() => {
  dispose?.(); dispose = undefined; forgetListReveal(); setSelectedKey(undefined); fixture.saved = undefined;
  document.body.replaceChildren(); vi.useRealTimers(); vi.unstubAllGlobals();
});

describe("Show in session list", () => {
  it("clears hiding search/status/scopes with a visible hint, opens the panel and preserves selection", async () => {
    put([row(1)], "", undefined, true); setSelectedKey(key(1)); setStateFilter("attention"); setSearch("not this session");
    setMachineFilter(["elsewhere"]); setProjectFilter(["beta"]); setAccountFilter(["another account"]);
    setFolderFilter({ computer: "", path: "/elsewhere", style: "posix", checkout: false }); setStripMode(true);
    mount(); await locate();
    expect([search(), stateFilter(), machineFilter(), projectFilter(), accountFilter(), folderFilter()]).toEqual(["", "all", [], [], [], undefined]);
    expect(stripMode()).toBe(false); expect(host.querySelector<HTMLElement>(".sp-panel")?.hidden).toBe(false);
    expect(selectedKey()).toBe(key(1)); expect(shownRow(key(1))?.getAttribute("aria-selected")).toBe("true");
    expect(host.querySelector('[role="status"]')?.textContent).toBe("Filters adjusted to show this session.");
    expect(fixture.scroll).toHaveBeenLastCalledWith(listRows().findIndex((r) => r.id === key(1)), { align: "center" });
    expect(fixture.inspect).not.toHaveBeenCalled(); expect(fixture.archive).not.toHaveBeenCalled();
  });

  it("keeps filters that already contain the selected session", async () => {
    put([row(2, { status: "working", activity: "busy", phase: "working" })]); setSelectedKey(key(2)); setStateFilter("active"); setProjectFilter(["alpha"]);
    mount(); await locate();
    expect(stateFilter()).toBe("active"); expect(projectFilter()).toEqual(["alpha"]);
    expect(host.querySelector(".sp-reveal-notice")).toBeNull(); expect(shownRow(key(2))).toBeTruthy();
  });

  it("waits for the existing project-layout batch before scrolling, and retires the filter hint", async () => {
    put([row(11)], "", undefined, true); setSelectedKey(key(11)); setSearch("hiding query");
    const expand = fixture.collapse; fixture.collapse = () => {};
    mount(); await locate();
    expect(fixture.scroll).not.toHaveBeenCalled();
    expect(host.querySelector('[role="status"]')).toBeTruthy();
    expand("alpha", false); await flush();
    expect(fixture.scroll).toHaveBeenCalledExactlyOnceWith(1, { align: "center" });
    vi.advanceTimersByTime(4999); expect(host.querySelector('[role="status"]')).toBeTruthy();
    vi.advanceTimersByTime(1); expect(host.querySelector(".sp-reveal-notice")).toBeNull();
  });

  it("drops a pending reveal when the person selects another conversation", async () => {
    put([row(12), row(13)], "", undefined, true); setSelectedKey(key(12));
    const expand = fixture.collapse; fixture.collapse = () => {};
    mount(); await locate(); setSelectedKey(key(13)); expand("alpha", false); await flush();
    expect(fixture.scroll).not.toHaveBeenCalled(); expect(listRevealPulse()).toBeUndefined();
    expect(selectedKey()).toBe(key(13));
  });

  it("expands the complete collapsed parent chain, including a collapsed project", async () => {
    const parent = row(3), child = row(4, { parent: { machine: MACHINE, session_id: uuid(3), name: parent.name, conversation_id: parent.conversation_id!, run_id: parent.run_id!, state: "running" } });
    const grandchild = row(5, { parent: { machine: MACHINE, session_id: uuid(4), name: child.name, conversation_id: child.conversation_id!, run_id: child.run_id!, state: "running" } });
    put([parent, child, grandchild], "", undefined, true); setTeamCollapsed(key(3), true); setTeamCollapsed(key(4), true); setSelectedKey(key(5));
    mount(); await locate();
    expect(teamCollapsed(key(3))).toBe(false); expect(teamCollapsed(key(4))).toBe(false);
    expect(shownRow(key(5))?.getAttribute("aria-selected")).toBe("true");
    expect(listRows().find((r) => r.id === key(5))).toMatchObject({ depth: 2 });
  });

  it("locates a selected native child without switching to its parent conversation", async () => {
    put([row(6, { subagents: { kid: { state: "working", label: "Child", inactive: false } } })], "", undefined, true);
    setTeamCollapsed(key(6), true); setSelectedKey(key(6)); setSubagentOpen("kid");
    mount(); await locate();
    expect(subagentOpen()).toBe("kid"); expect(selectedKey()).toBe(key(6));
    expect(shownRow(`${key(6)}/kid`)?.getAttribute("aria-selected")).toBe("true");
    expect(listRevealPulse()?.row).toBe(`${key(6)}/kid`);
  });

  it("locates remote sessions using their exact computer/key without an inspection", async () => {
    put([row(7)], "peer", undefined, true); setSelectedKey(key(7, "peer")); mount(); await locate();
    expect(shownRow(key(7, "peer"))).toBeTruthy(); expect(selectedKey()).toBe(key(7, "peer")); expect(fixture.inspect).not.toHaveBeenCalled();
  });

  it("opens the lead's project for an Agent Team member grouped there", async () => {
    const members = teamFixture.team.members.filter((m) => m.session);
    const input = members.map((m, n) => row(20 + n, { launch_id: m.session!.session_id.slice("launch_id:".length) }));
    put(input, "", members.map((m) => m.id === teamFixture.list.teams[0].lead ? "alpha" : "beta"), true);
    teams.apply([{ computer: "", key: "list", kind: "list", payload: teamFixture.list }]);
    const target = input[1]; setSelectedKey(`\nrun:${target.run_id}`); mount(); await locate();
    expect(shownRow(selectedKey()!)).toBeTruthy();
    const rows = listRows(), targetIndex = rows.findIndex((r) => r.id === selectedKey());
    expect(rows.slice(0, targetIndex).filter((r) => r.type === "header").at(-1)).toMatchObject({ project: "alpha", collapsed: false });
  });

  it("shows a known archived selection outside the loaded page without reading history or losing it", async () => {
    put([], "", [], true);
    const saved = toSession("peer", "archive:old", row(8, { state: "archived", archive_id: uuid(80) }));
    fixture.saved = { ...saved, group: "alpha", archivedAt: 1 } as Session;
    setSelectedKey(saved.key); mount(); await locate();
    expect(stateFilter()).toBe("archived"); expect(selected()?.archiveId).toBe(uuid(80));
    expect(shownRow(saved.key)?.getAttribute("aria-selected")).toBe("true");
    expect(host.querySelector('[role="status"]')?.textContent).toContain("outside the current archive page");
    expect(fixture.archive).not.toHaveBeenCalled(); expect(fixture.inspect).not.toHaveBeenCalled();
    setSelectedKey(undefined); await flush();
    expect(listRows().filter((r) => r.type === "card")).toHaveLength(0);
  });

  it("restarts two pulses on another click, then restores the selected style", async () => {
    put([row(9)]); setSelectedKey(key(9)); mount(); await locate();
    const first = listRevealPulse()!.serial, initial = shownRow(key(9))!;
    expect(initial.classList.contains(first % 2 ? "locate-pulse-b" : "locate-pulse-a")).toBe(true);
    vi.advanceTimersByTime(1000); await locate();
    expect(listRevealPulse()!.serial).toBeGreaterThan(first);
    vi.advanceTimersByTime(1499); expect(listRevealPulse()).toBeTruthy();
    vi.advanceTimersByTime(1); expect(listRevealPulse()).toBeUndefined();
    expect(initial.className).not.toMatch(/locate-pulse/); expect(initial.getAttribute("aria-selected")).toBe("true");
  });

  it("uses a static attention highlight when the system requests reduced motion", async () => {
    put([row(10)]); setSelectedKey(key(10)); fixture.media?.({ matches: true }); mount(); await locate();
    expect(shownRow(key(10))?.classList.contains("locate-static")).toBe(true);
    vi.advanceTimersByTime(1500); expect(shownRow(key(10))?.classList.contains("locate-static")).toBe(false);
  });

  it("centers a distant row in a 20,000-session list while mounting only the virtual window", async () => {
    const input = Array.from({ length: 20_000 }, (_, n) => row(100 + n));
    put(input); setSelectedKey(key(20_099)); mount(); await locate();
    expect(fixture.scroll).toHaveBeenLastCalledWith(20_000, { align: "center" });
    expect(host.querySelectorAll(".vrow").length).toBeLessThanOrEqual(6);
    expect(shownRow(key(20_099))).toBeTruthy(); expect(fixture.inspect).not.toHaveBeenCalled();
  }, 20_000);
});
