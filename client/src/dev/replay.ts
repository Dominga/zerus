import { emptyBatch, type ActivityEvent, type Batch, type ComputerOut, type MachineOut, type ReplaceOut, type RowOut, type KeyOut } from "../types";
import type { FollowRequest } from "../store/activity";

// Browser-only stand-in for the Rust side, for `npm run dev` without Tauri.
// It replays fixtures/replay-batches.json, which scripts/make-fixture.py
// writes from the same synthetic fleet as the watch stream fixture the app
// replays in Rust (ZERUS_CLIENT_REPLAY). It is never used inside Tauri unless
// the app was built without its live data source.

interface Fixture {
  interval_ms: number;
  computers: ComputerOut[];
  machines?: MachineOut[];
  snapshot: ReplaceOut[];
  deltas: { upserts?: RowOut[]; removes?: KeyOut[] }[];
  activity: Record<string, { events: ActivityEvent[]; cursor: number; state?: Record<string, unknown> }>;
}

const PARK_MS = 250;
const READ = 200;

export function createReplay() {
  let fixture: Fixture | undefined;
  let pending: Batch = emptyBatch();
  let dirty = false;
  let wake: (() => void) | undefined;
  let tick = 0;

  const push = (part: Partial<Batch>) => {
    for (const k of Object.keys(part) as (keyof Batch)[]) (pending[k] as unknown[]).push(...((part[k] ?? []) as unknown[]));
    dirty = true;
    wake?.();
  };

  void import("../../fixtures/replay-batches.json").then((m) => {
    fixture = m.default as unknown as Fixture;
    push({ computers: fixture.computers, machines: fixture.machines ?? [], replaces: fixture.snapshot });
    setInterval(() => {
      if (!fixture?.deltas.length) return;
      const delta = fixture.deltas[tick++ % fixture.deltas.length];
      push({ upserts: delta.upserts ?? [], removes: delta.removes ?? [] });
    }, fixture.interval_ms);
  });

  return {
    async next(): Promise<Batch> {
      if (!dirty) await new Promise<void>((resolve) => { wake = resolve; setTimeout(resolve, PARK_MS); });
      wake = undefined;
      const out = pending;
      pending = emptyBatch();
      dirty = false;
      return out;
    },
    async follow(r: FollowRequest) {
      const log = fixture?.activity[`${r.computer}\n${r.key}`];
      if (!log) { push({ activity: [{ computer: r.computer, key: r.key, kind: "events", payload: { events: [], cursor: 0 } }] }); return; }
      // As the stream does: the newest `tail` events, or the events after `after` in reads of 200.
      const events = r.after === undefined ? log.events.slice(-Math.min(r.tail ?? 100, 100))
        : log.events.filter((e) => (e.seq ?? 0) > r.after!);
      const reads = r.after === undefined ? [events] : Array.from({ length: Math.ceil(events.length / READ) }, (_, i) => events.slice(i * READ, (i + 1) * READ));
      for (const read of reads) push({ activity: [{ computer: r.computer, key: r.key, kind: "events", payload: { events: read, cursor: read.at(-1)?.seq ?? log.cursor } }] });
      if (log.state) push({ activity: [{ computer: r.computer, key: r.key, kind: "state", payload: { value: log.state, rev: 1 } }] });
    },
    async unfollow() {},
  };
}
