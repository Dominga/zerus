import { describe, expect, it, vi } from "vitest";
import { createRoot } from "solid-js";
import { createCatalog, toSession } from "./catalog";
import { connectionOf, createOverview, fleetCards, liveSessions, mainSessions, overviewStats } from "./overview";
import { machineMeta, rowOut } from "./testing";
import { emptyBatch } from "../types";
import type { RowMarks, RowsPort } from "./marks";

const live = (id: string) => ({ id, status: "live", attempt: 0 });

describe("overview selectors", () => {
  it("map the stream states as the rulings say", () => {
    expect(["live", "polled", "probing", "reconnecting", "unsupported", "ended", undefined].map((s) => connectionOf(s))).toEqual([
      { tile: "online", text: "Connected", fresh: true },
      { tile: "online", text: "Update hgs", fresh: true },
      { tile: "connecting", text: "Connecting", fresh: false },
      { tile: "connecting", text: "Reconnecting", fresh: false },
      { tile: "offline", text: "Update hgs", fresh: false },
      { tile: "offline", text: "Offline", fresh: false },
      { tile: "offline", text: "Offline", fresh: false },
    ]);
  });

  it("list this computer unless it is a control console, then the peers that have not ended", () => {
    const computers = { "": live(""), studio: { id: "studio", status: "reconnecting", attempt: 1 }, old: { id: "old", status: "ended", attempt: 0 }, alpha: { id: "alpha", status: "probing", attempt: 0 } };
    const cards = fleetCards(computers, { "": machineMeta("workstation"), studio: machineMeta("studio", {}, { platform: "macos" }) });
    expect(cards.map((c) => [c.name, c.filter, c.local, c.platform])).toEqual([
      ["workstation", "workstation", true, "linux"], ["alpha", "alpha", false, undefined], ["studio", "studio", false, "macos"],
    ]);
    const console = fleetCards(computers, { "": machineMeta("workstation", {}, { local_sessions: false }) });
    expect(console.map((c) => c.name)).toEqual(["alpha", "studio"]);
  });

  it("order the live sessions attention first, then the latest event, then the key", () => {
    const rows = [
      toSession("", "run:1", rowOut("", 1, { status: "working", updated_at: 10 }).row),
      toSession("", "run:2", rowOut("", 2, { status: "attention", phase: "approval", updated_at: 5 }).row),
      toSession("", "run:3", rowOut("", 3, { status: "working", updated_at: 20 }).row),
      toSession("", "run:4", rowOut("", 4, { status: "ready" }).row),
      toSession("", "run:0", rowOut("", 0, { status: "working", updated_at: 20 }).row),
    ];
    expect(liveSessions(rows, { "": live("") }).map((s) => s.catalogKey)).toEqual(["run:2", "run:0", "run:3", "run:1"]);
  });

  it("Main only drops a child only under a shown card, and keeps one that needs the person", () => {
    const row = (n: number, fields: Record<string, unknown>) => toSession("", `run:${n}`, rowOut("", n, { status: "working", ...fields }).row);
    const lead = row(1, {});
    const child = row(2, {});
    const grandchild = row(3, {});
    const asking = row(4, { status: "attention", phase: "approval", needs_action: true, needs_attention: true, human_request: true });
    const orphan = row(5, {});
    const loopA = row(6, {});
    const loopB = row(7, {});
    const parents: Record<string, string> = {
      [child.key]: lead.key, [grandchild.key]: child.key, [asking.key]: lead.key,
      [orphan.key]: "idle-parent-not-on-the-overview", [loopA.key]: loopB.key, [loopB.key]: loopA.key,
    };
    const shown = mainSessions([lead, child, grandchild, asking, orphan, loopA, loopB], (s) => parents[s.key]);
    expect(shown.map((s) => s.key)).toEqual([lead.key, asking.key, orphan.key, loopA.key, loopB.key]);
    // A quiet supervised child's request is its parent's: it drops out with the others.
    const quiet = { ...row(8, { status: "attention", phase: "approval", needs_action: true, needs_attention: true }), quiet: true };
    expect(mainSessions([lead, quiet], (s) => (s.key === quiet.key ? lead.key : undefined)).map((s) => s.key)).toEqual([lead.key]);
    // ...unless the person marked it Review later, which the attention tile counts.
    const marked = { ...quiet, reviewLater: true };
    expect(mainSessions([lead, marked], (s) => (s.key === marked.key ? lead.key : undefined)).map((s) => s.key)).toEqual([lead.key, marked.key]);
    // A long chain under one card stays linear and drops every link.
    const chain = Array.from({ length: 20_000 }, (_, i) => ({ ...lead, key: `chain:${i}` }));
    const started = performance.now();
    expect(mainSessions(chain, (s) => { const i = Number(s.key.slice(6)); return i ? `chain:${i - 1}` : undefined; }).map((s) => s.key)).toEqual(["chain:0"]);
    expect(performance.now() - started).toBeLessThan(1_000);
  });

  it("sort by Qt's session.needsAttention(), so an asking subagent does not go first (DashboardPage.cpp:453)", () => {
    const rows = [
      toSession("", "run:1", rowOut("", 1, { status: "attention", needs_action: false, needs_attention: false, child_attention: true, updated_at: 30 }).row),
      toSession("", "run:2", rowOut("", 2, { status: "attention", phase: "approval", needs_action: true, needs_attention: true, updated_at: 5 }).row),
    ];
    expect(liveSessions(rows, { "": live("") }).map((s) => s.catalogKey)).toEqual(["run:2", "run:1"]);
  });

  it("count the total by the window's rule and a machine card by Qt's session rule (DashboardPage.cpp:133, :426)", () => {
    const asking = (i: number, computer = "") => rowOut(computer, i, { status: "attention", phase: "approval", needs_action: true, needs_attention: true });
    const rows = [
      // A subagent asks: the total counts it, its machine card does not.
      toSession("", "run:1", rowOut("", 1, { status: "attention", needs_attention: false, child_attention: true }).row),
      toSession("", "run:2", asking(2).row),
      // An unreachable peer: its card still counts the request, the total only Review later.
      toSession("studio", "run:3", asking(3, "studio").row),
      toSession("studio", "run:4", rowOut("studio", 4, { needs_attention: false }).row),
      // A muted request counts nowhere.
      toSession("", "run:5", rowOut("", 5, { status: "attention", phase: "approval", needs_action: true, needs_attention: true, muted: true }).row),
    ];
    // Review later, as store/marks.ts overlays it.
    rows[3] = { ...rows[3], reviewLater: true, attentionNeeded: true };
    const stats = overviewStats(rows, { "": live(""), studio: { id: "studio", status: "reconnecting", attempt: 1 } });
    expect(stats.attention).toBe(3);
    expect(stats.per[""]).toEqual({ sessions: 3, attention: 1 });
    expect(stats.per.studio).toEqual({ sessions: 2, attention: 2 });
    expect(stats.offline).toBe(2);
    // An offline peer's request is not a live session; its Review later is.
    expect(liveSessions(rows, { "": live(""), studio: { id: "studio", status: "reconnecting", attempt: 1 } }).map((s) => s.catalogKey))
      .toEqual(["run:5", "run:4", "run:2", "run:1"]);
  });

  it("follow every catalog change once, and render live sessions as store rows", () => {
    createRoot((dispose) => {
      const { catalog, applyBatch, setDraftKeys, plainRows, plainComputers } = createCatalog();
      applyBatch({ ...emptyBatch(), computers: [{ computer: "", status: "live", attempt: 0 }], replaces: [{ computer: "", rows: [0, 1, 2, 3].map((i) => rowOut("", i)) }] });
      const compute = vi.fn(overviewStats);
      const ov = createOverview(plainRows, plainComputers, () => ({}), compute, (s) => catalog.rows[s.key] ?? s);
      expect(ov.stats()).toMatchObject({ all: 4, attention: 0, active: 0, offline: 0 });
      expect(ov.live()).toEqual([]);
      // A field change in place (an upsert reconciles the existing row).
      applyBatch({ ...emptyBatch(), upserts: [rowOut("", 1, { status: "attention", phase: "approval" })] });
      expect(ov.stats().attention).toBe(1);
      expect(compute).toHaveBeenCalledTimes(2);
      // A live card renders the store's row, so its fields stay reactive.
      const [card] = ov.live();
      expect(card).toBe(catalog.rows[card.key]);
      expect(card.status).toBe("attention");
      // A draft makes a ready session live: one recompute for the whole change.
      setDraftKeys(["\nclaude/sample-project/t3"]);
      expect(ov.live().map((s) => s.catalogKey)).toEqual(["run:1", "run:3"]);
      expect(compute).toHaveBeenCalledTimes(3);
      // A computer going offline changes the tiles too.
      applyBatch({ ...emptyBatch(), computers: [{ computer: "", status: "reconnecting", attempt: 1 }] });
      expect(ov.stats().offline).toBe(4);
      expect(ov.stats()).toEqual(overviewStats(catalog.order.map((id) => catalog.rows[id]), catalog.computers));
      expect(compute).toHaveBeenCalledTimes(4);
      // A removal, and an empty batch that changes nothing.
      applyBatch({ ...emptyBatch(), removes: [{ computer: "", key: "run:0" }] });
      expect(ov.stats().all).toBe(3);
      applyBatch(emptyBatch());
      expect(compute).toHaveBeenCalledTimes(5);
      dispose();
    });
  });

  // review-q's probes on #1299: the two catalog paths no other test pins.
  it("a marks overlay patch reaches the tiles and the live order", () => {
    createRoot((dispose) => {
      let port: RowsPort | undefined;
      const fake = { overlay: () => ({}), attach: (p: RowsPort) => { port = p; } } as unknown as RowMarks;
      const { catalog, applyBatch, plainRows, plainComputers } = createCatalog(fake);
      applyBatch({ ...emptyBatch(), computers: [{ computer: "", status: "live", attempt: 0 }], replaces: [{ computer: "", rows: [0, 1, 2].map((i) => rowOut("", i)) }] });
      const compute = vi.fn(overviewStats);
      const ov = createOverview(plainRows, plainComputers, () => ({}), compute, (s) => catalog.rows[s.key] ?? s);
      const id = catalog.order[1];
      const row = catalog.rows[id];
      port!.patch(id, { status: row.status, summary: row.summary, unread: row.unread, caption: row.caption, attentionNeeded: true });
      expect(ov.stats().attention).toBe(1);
      expect(ov.live()[0]).toBe(catalog.rows[id]);
      expect(compute).toHaveBeenCalledTimes(2);
      dispose();
    });
  });

  it("the machine cards follow a computer going offline", () => {
    createRoot((dispose) => {
      const { applyBatch, plainRows, plainComputers } = createCatalog();
      applyBatch({ ...emptyBatch(), computers: [{ computer: "", status: "live", attempt: 0 }], replaces: [{ computer: "", rows: [rowOut("", 0)] }] });
      const ov = createOverview(plainRows, plainComputers, () => ({}));
      expect(ov.cards()[0].connection.fresh).toBe(true);
      applyBatch({ ...emptyBatch(), computers: [{ computer: "", status: "reconnecting", attempt: 1 }] });
      expect(ov.cards()[0].connection.fresh).toBe(false);
      dispose();
    });
  });

  it("update the tiles once per batch for 20,000 rows", () => {
    createRoot((dispose) => {
      const { catalog, applyBatch, plainRows, plainComputers } = createCatalog();
      const rows = Array.from({ length: 20_000 }, (_, i) => rowOut(i % 2 ? "studio" : "", i));
      applyBatch({ ...emptyBatch(), computers: [live(""), { id: "studio", status: "reconnecting", attempt: 1 }].map((c) => ({ computer: c.id, status: c.status, attempt: c.attempt })),
        replaces: [{ computer: "", rows: rows.filter((r) => r.computer === "") }, { computer: "studio", rows: rows.filter((r) => r.computer === "studio") }] });
      const compute = vi.fn(overviewStats);
      // As the Overview does: the catalog's plain rows, one dependency per batch.
      const ov = createOverview(plainRows, plainComputers, () => ({}), compute, (s) => catalog.rows[s.key] ?? s);
      expect(ov.stats()).toMatchObject({ all: 20_000, attention: 0, offline: 10_000 });
      expect(compute).toHaveBeenCalledTimes(1);
      applyBatch({ ...emptyBatch(), upserts: Array.from({ length: 100 }, (_, i) => rowOut("", i * 2, { status: "attention", phase: "approval" })) });
      expect(ov.stats().attention).toBe(100);
      expect(ov.stats().per[""]).toEqual({ sessions: 10_000, attention: 100 });
      expect(ov.stats().per.studio).toEqual({ sessions: 10_000, attention: 0 });
      expect(compute).toHaveBeenCalledTimes(2);
      dispose();
    });
  });
});
