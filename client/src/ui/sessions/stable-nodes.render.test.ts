// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from "vitest";
import { createComponent } from "solid-js";
import { render } from "solid-js/web";

// zerus-92hkz (owner, Arch): icon tooltips blinked every 1-2 s while
// hovering. WebKitGTK restarts a native title tooltip when its element is
// replaced, so an unchanged catalog refresh must keep every titled element
// of the session list and the selection bar the same node.
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
import { compactRows, now, setCompactRows, setMulti, setNow, setPage, setStripMode } from "../../store/ui";

let dispose: (() => void) | undefined;
const settle = async () => { for (let i = 0; i < 12; i++) await Promise.resolve(); };
afterEach(async () => { dispose?.(); dispose = undefined; await settle(); document.body.replaceChildren(); setMulti({}); vi.restoreAllMocks(); vi.unstubAllGlobals(); });

// The periodic refresh: what the bar and the row icons show is unchanged,
// while fields the core reports for a working agent move every tick (its
// update time and activity summary).
const batch = (tick = 0) => ({ ...emptyBatch(), replaces: [{ computer: "", rows: [
  { computer: "", key: "run:stable-0", row: { name: "codex/stable/0", agent: "codex", status: "working", state: "running", activity: "busy", phase: "working",
    turn_started: 1_793_000_000, cwd: "/stable/0", run_id: "stable-0", conversation_id: "c0", unread: true, updated_at: 1_793_000_000 + tick, summary: `Working ${tick}` } },
  { computer: "", key: "run:stable-1", row: { name: "claude/stable/1", agent: "claude", status: "ready", state: "running", activity: "idle", cwd: "/stable/1", run_id: "stable-1",
    conversation_id: "c1", optional_question_count: 1, updated_at: 1_793_000_000 + tick, summary: `Ready ${tick}`,
    subagents: { "agent-a": { state: "working", label: "Reviewer", detail: `step ${tick}` } } } },
] }] });

describe("an unchanged catalog refresh (zerus-92hkz)", () => {
  it.each([false, true])("keeps every titled element of the rows (compact %s) and the selection bar the same node", async (compact) => {
    // The bar measures its width to show its tools; give it room for all of them.
    vi.stubGlobal("ResizeObserver", class { constructor(private run: () => void) {} observe() { this.run(); } disconnect() {} });
    vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockReturnValue(1200);
    setPage("sessions"); setStripMode(false);
    applyBatch(batch());
    setMulti({ "\nrun:stable-0": true, "\nrun:stable-1": true });
    for (const key of ["\nrun:stable-0", "\nrun:stable-1"]) setCompactRows(key, compact);
    const host = document.createElement("div"); document.body.append(host);
    dispose = render(() => createComponent(SessionList, {}), host);
    await settle();
    const titled = () => [...host.querySelectorAll<HTMLElement>("[title], [data-tip]")];
    const before = titled();
    expect(!!compactRows["\nrun:stable-0"]).toBe(compact);
    expect(host.querySelector(".sel-bar, .selection-bar, [data-selection-bar]") ?? before.find((e) => e.getAttribute("aria-label") === "Deselect all")).toBeTruthy();
    expect(before.length).toBeGreaterThan(4);
    // The bar's own tool buttons show, not only its overflow menu.
    expect(before.some((e) => /of 2 selected sessions/.test(e.getAttribute("title") ?? ""))).toBe(true);
    const removed: string[] = [];
    const observer = new MutationObserver((records) => { for (const r of records) for (const n of r.removedNodes) if (n instanceof HTMLElement && (n.matches("[title], [data-tip]") || n.querySelector("[title], [data-tip]"))) removed.push(n.getAttribute("title") ?? n.className); });
    const retitled: string[] = [];
    const titles = new MutationObserver((records) => { for (const r of records) if (r.target instanceof HTMLElement) retitled.push(`${r.oldValue} -> ${r.target.getAttribute(r.attributeName!)}`); });
    observer.observe(host, { childList: true, subtree: true });
    titles.observe(host, { attributes: true, attributeOldValue: true, attributeFilter: ["title", "data-tip"], subtree: true });
    // Each tick also advances the clock the Working timers read.
    for (let tick = 1; tick <= 3; tick++) { setNow(now() + 1); applyBatch(batch(tick)); await settle(); }
    observer.disconnect(); titles.disconnect();
    // Only a child's own changing detail may change a title here.
    expect(retitled.filter((change) => !/^step \d -> step \d$/.test(change))).toEqual([]);
    expect(removed).toEqual([]);
    const lost = before.filter((e) => !e.isConnected).map((e) => e.getAttribute("title") ?? e.getAttribute("data-tip"));
    expect(lost).toEqual([]);
  });
});
