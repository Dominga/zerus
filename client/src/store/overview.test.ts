import { describe, expect, it, vi } from "vitest";
import { createRoot } from "solid-js";
import { createCatalog, toSession } from "./catalog";
import { connectionOf, createOverview, fleetCards, liveSessions, overviewStats } from "./overview";
import { machineMeta, rowOut } from "./testing";
import { emptyBatch } from "../types";

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

  it("update the tiles once per batch for 20,000 rows", () => {
    createRoot((dispose) => {
      const { catalog, applyBatch } = createCatalog();
      const rows = Array.from({ length: 20_000 }, (_, i) => rowOut(i % 2 ? "studio" : "", i));
      applyBatch({ ...emptyBatch(), computers: [live(""), { id: "studio", status: "reconnecting", attempt: 1 }].map((c) => ({ computer: c.id, status: c.status, attempt: c.attempt })),
        replaces: [{ computer: "", rows: rows.filter((r) => r.computer === "") }, { computer: "studio", rows: rows.filter((r) => r.computer === "studio") }] });
      const compute = vi.fn(overviewStats);
      const ov = createOverview(() => catalog.order.map((id) => catalog.rows[id]), () => catalog.computers, () => ({}), compute);
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
