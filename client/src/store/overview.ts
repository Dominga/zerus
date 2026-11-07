import { createMemo, untrack, type Accessor } from "solid-js";
import type { ComputerState } from "./catalog";
import { attentionNeeded, countedAttention, isActive, quietAttention } from "./attention";
import type { ComputerId, MachineMeta } from "../types";
import type { Session } from "../ui/sessions/types";
import type { MachineState } from "../ui/components/MachineTile";
import type { OsKind } from "../ui/lib/os-icons";
import { tr } from "../i18n";

// The Overview's selectors (DashboardPage::render, DashboardPage.cpp:401-456).
// The Overview screen creates them, so they cost nothing on other pages; each
// is one pass over the catalog per batch and none runs on a timer.

/** A machine's connection as the Overview and Machines show it (Decisions note). */
export interface Connection { tile: MachineState; text: string; fresh: boolean }

/** Only a live stream is fresh: the "Machines online" tile, the load colours and the offline notice count it. */
export function connectionOf(status: string | undefined): Connection {
  switch (status) {
    case "live": return { tile: "online", text: "Connected", fresh: true };
    case "polled": return { tile: "online", text: tr("Update hgs"), fresh: true };
    case "probing": return { tile: "connecting", text: "Connecting", fresh: false };
    case "reconnecting": return { tile: "connecting", text: "Reconnecting", fresh: false };
    case "unsupported": return { tile: "offline", text: tr("Update hgs"), fresh: false };
    default: return { tile: "offline", text: "Offline", fresh: false };
  }
}

export const osOf = (platform: string | null | undefined): OsKind | undefined =>
  platform === "linux" || platform === "macos" || platform === "windows" ? platform : undefined;

export interface MachineCardModel {
  computer: ComputerId;
  /** The badge: this computer's host name, a peer's alias. */
  name: string;
  /** The Sessions machine filter's value for this computer's rows. */
  filter: string;
  local: boolean;
  platform?: OsKind;
  connection: Connection;
  meta?: MachineMeta;
}

/** This computer unless it is a control console (DashboardPage.cpp:407-409), then every peer that has not ended. */
export function fleetCards(computers: Record<ComputerId, ComputerState>, cards: Record<ComputerId, MachineMeta>): MachineCardModel[] {
  const out: MachineCardModel[] = [];
  const localMeta = cards[""];
  if ((computers[""] || localMeta) && localMeta?.local_sessions !== false) {
    out.push({
      computer: "", name: localMeta?.host || tr("This machine"), filter: localMeta?.host || "this computer", local: true,
      platform: osOf(localMeta?.platform), connection: connectionOf(computers[""]?.status), meta: localMeta,
    });
  }
  for (const id of Object.keys(computers).filter((id) => id !== "" && computers[id].status !== "ended").sort()) {
    out.push({ computer: id, name: id, filter: id, local: false, platform: osOf(cards[id]?.platform), connection: connectionOf(computers[id].status), meta: cards[id] });
  }
  return out;
}

export interface OverviewStats {
  all: number;
  active: number;
  attention: number;
  /** Rows on machines whose stream is not live (the offline notice). */
  offline: number;
  per: Record<ComputerId, { sessions: number; attention: number }>;
}

/** A computer's rows are current: its stream is live or polled (Qt's `machine.fresh`). */
const fresh = (computers: Record<ComputerId, ComputerState>, computer: ComputerId) =>
  connectionOf(computers[computer]?.status).fresh;

/**
 * The total follows the window's rule on current machines
 * (`SessionPresentation::needsAttention(session, machine.fresh)`,
 * DashboardPage.cpp:426-427); a machine card counts Qt's
 * `session.needsAttention()` (DashboardPage.cpp:133), without subagents
 * and whatever its freshness.
 */
export function overviewStats(rows: readonly Session[], computers: Record<ComputerId, ComputerState>): OverviewStats {
  const stats: OverviewStats = { all: 0, active: 0, attention: 0, offline: 0, per: {} };
  for (const s of rows) {
    const per = (stats.per[s.computer] ??= { sessions: 0, attention: 0 });
    const current = fresh(computers, s.computer);
    stats.all++;
    per.sessions++;
    if (countedAttention(s, current)) stats.attention++;
    if (!quietAttention(s) && attentionNeeded(s)) per.attention++;
    if (isActive(s, current)) stats.active++;
    if (!current) stats.offline++;
  }
  return stats;
}

/**
 * Working, attention and draft sessions of current machines' activity:
 * `session.needsAttention()` first (DashboardPage.cpp:453), then the latest
 * event, then the key (DashboardPage.cpp:452-456).
 */
export function liveSessions(rows: readonly Session[], computers: Record<ComputerId, ComputerState>): Session[] {
  return rows.filter((s) => isActive(s, fresh(computers, s.computer))).sort((a, b) =>
    Number(attentionNeeded(b)) - Number(attentionNeeded(a))
    || (b.updatedAt ?? 0) - (a.updatedAt ?? 0)
    || (a.key < b.key ? -1 : a.key > b.key ? 1 : 0));
}

/**
 * The Overview's Main only cards (owner 2026-11-07): a child session drops out
 * only while its chain of live parents ends at a card shown here, which stands
 * for its team. A child whose own request needs the person (not quiet) always
 * stays, and so do a child whose parent is not live here and a parent loop.
 * `parentOf` is the session list's parent lookup (`parentKeyOf`).
 */
export function mainSessions(live: readonly Session[], parentOf: (s: Session) => string | undefined): Session[] {
  const byKey = new Map(live.map((s) => [s.key, s]));
  const liveParent = (s: Session) => { const key = parentOf(s); return key === undefined ? undefined : byKey.get(key); };
  // Whether a row's chain of live parents ends at a row without one; a loop never
  // does. Each walk records its whole path, so the pass stays linear.
  const ends = new Map<string, boolean>();
  const rooted = (s: Session) => {
    const path: string[] = [], onPath = new Set<string>();
    let result = false;
    for (let at: Session | undefined = s; at; ) {
      const known = ends.get(at.key);
      if (known !== undefined) { result = known; break; }
      if (onPath.has(at.key)) break;
      onPath.add(at.key); path.push(at.key);
      at = liveParent(at);
      if (!at) result = true;
    }
    for (const key of path) ends.set(key, result);
    return result;
  };
  // The person's own Review later mark counts even on a quiet child (attention.ts quietCounts).
  const ownMark = (s: Session) => !s.muted && !!s.reviewLater;
  return live.filter((s) => !liveParent(s) || (attentionNeeded(s) && !quietAttention(s)) || ownMark(s) || !rooted(s));
}

/**
 * The Overview's selectors, each recomputed once per catalog batch. Give
 * them the catalog's plain rows and computers (`plainRows`, `plainComputers`),
 * which track only its revision: reading every row through the store would
 * subscribe to each field of tens of thousands of rows on every batch
 * (zerus-5wmh.66.50). `view` maps a live session back to the row a card
 * renders reactively (the store's `catalog.rows[s.key]`).
 */
export function createOverview(
  rows: Accessor<readonly Session[]>,
  computers: Accessor<Record<ComputerId, ComputerState>>,
  cards: Accessor<Record<ComputerId, MachineMeta>>,
  compute: typeof overviewStats = overviewStats,
  view: (s: Session) => Session = (s) => s,
) {
  return {
    stats: createMemo(() => compute(rows(), computers())),
    live: createMemo(() => { const live = liveSessions(rows(), computers()); return untrack(() => live.map(view)); }),
    cards: createMemo(() => fleetCards(computers(), cards())),
  };
}
