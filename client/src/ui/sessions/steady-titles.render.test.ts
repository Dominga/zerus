// @vitest-environment happy-dom
import { afterAll, afterEach, describe, expect, it, vi } from "vitest";
import { createComponent } from "solid-js";
import { render } from "solid-js/web";

// zerus-5ryrx (the rest of the owner's zerus-92hkz report): a working row's
// tooltip carried its Working time and changed every second, and WebKitGTK
// restarts a native tooltip whose text changes. A tooltip never counts
// seconds, and the one under the pointer holds still; the row's visible
// status keeps its live seconds.
vi.hoisted(() => { vi.useFakeTimers({ toFake: ["Date"], now: (1_793_000_000 + 9 * 60 + 57) * 1000 }); });
vi.mock("../../bridge", async (original) => ({ ...await original<object>(), gitRows: vi.fn().mockResolvedValue(undefined),
  windowVisibility: { hidden: () => false, onChange: () => () => {} },
}));
vi.mock("@tanstack/solid-virtual", () => ({
  createVirtualizer: (options: { count: number; getItemKey(n: number): string }) => ({
    measure() {}, measureElement() {}, _willUpdate() {}, scrollToIndex() {}, getTotalSize: () => options.count * 94,
    getVirtualItems: () => Array.from({ length: options.count }, (_, index) => ({ key: options.getItemKey(index), start: index * 94, index })),
  }),
}));
import { SessionList } from "./SessionList";
import { applyBatch } from "../../store/catalog";
import { emptyBatch } from "../../types";
import { now, setCompactRows, setNow, setPage, setStripMode } from "../../store/ui";

const TURN_STARTED = 1_793_000_000;
/** 9 min 57 s into both rows' turn: the third tick reaches 10 min. */
const CLOCK = TURN_STARTED + 9 * 60 + 57;

let dispose: (() => void) | undefined;
const settle = async () => { for (let i = 0; i < 12; i++) await Promise.resolve(); };
afterEach(async () => { dispose?.(); dispose = undefined; await settle(); document.body.replaceChildren(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });
afterAll(() => { vi.useRealTimers(); });

const working = (n: number) => ({ computer: "", key: `run:steady-${n}`, row: { name: `codex/steady/${n}`, agent: "codex", status: "working", state: "running",
  activity: "busy", phase: "working", turn_started: TURN_STARTED, cwd: `/steady/${n}`, run_id: `steady-${n}`, conversation_id: `c${n}`, updated_at: TURN_STARTED } });

async function mount(strip: boolean) {
  vi.stubGlobal("ResizeObserver", class { constructor(private run: () => void) {} observe() { this.run(); } disconnect() {} });
  vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockReturnValue(1200);
  setPage("sessions"); setStripMode(strip); setNow(CLOCK);
  applyBatch({ ...emptyBatch(), replaces: [{ computer: "", rows: [working(0), working(1)] }] });
  for (const n of [0, 1]) setCompactRows(`\nrun:steady-${n}`, true);
  const host = document.createElement("div"); document.body.append(host);
  dispose = render(() => createComponent(SessionList, {}), host);
  await settle();
  return host;
}
const tick = async (count = 1) => { for (let i = 0; i < count; i++) { setNow(now() + 1); await settle(); } };
const hover = (element: Element, on: boolean) => element.dispatchEvent(new Event(on ? "pointerenter" : "pointerleave"));

describe("a working row's tooltip under the pointer", () => {
  it("never counts seconds, holds across 1 s ticks and a minute boundary while hovered, and an unhovered row updates", async () => {
    const host = await mount(false);
    const rows = [...host.querySelectorAll(".scard.compact")];
    expect(rows).toHaveLength(2);
    const titled = (row: Element) => [row.querySelector(".sc-title")!, row.querySelector(".cc-expand")!];
    const titles = (row: Element) => titled(row).map((e) => e.getAttribute("title") ?? "");
    for (const title of [...titles(rows[0]), ...titles(rows[1])]) {
      expect(title).toContain("Working (9m)");
      expect(title, "no seconds counter").not.toMatch(/\d+s\)/);
    }
    // The visible status keeps its live seconds.
    expect(rows[0].querySelector(".sb-label")?.textContent).toBe("Working (9m 57s)");
    const [hoveredTitle, hoveredExpand] = titled(rows[0]);
    hover(hoveredTitle, true); hover(hoveredExpand, true);
    const held = titles(rows[0]);
    await tick(2);
    expect(rows[0].querySelector(".sb-label")?.textContent).toBe("Working (9m 59s)");
    expect(titles(rows[0])).toEqual(held);
    await tick(3);
    // Past the minute: the unhovered row's tooltip and the visible status move on; the hovered one holds.
    expect(rows[0].querySelector(".sb-label")?.textContent).toBe("Working (10m 02s)");
    expect(titles(rows[0])).toEqual(held);
    for (const title of titles(rows[1])) expect(title).toContain("Working (10m)");
    hover(hoveredTitle, false); hover(hoveredExpand, false);
    for (const title of titles(rows[0])) expect(title, "after the pointer leaves").toContain("Working (10m)");
  });

  it("holds a strip tile's tooltip the same way", async () => {
    const host = await mount(true);
    const tiles = [...host.querySelectorAll<HTMLElement>(".strip-tile")];
    expect(tiles).toHaveLength(2);
    expect(tiles[0].title).toContain("codex/steady/0: Working (9m)");
    hover(tiles[0], true);
    const held = tiles[0].title;
    await tick(5);
    expect(tiles[0].title).toBe(held);
    expect(tiles[1].title).toContain("codex/steady/1: Working (10m)");
    expect(tiles[1].title).not.toMatch(/\d+s\)/);
    hover(tiles[0], false);
    expect(tiles[0].title).toContain("Working (10m)");
  });
});
