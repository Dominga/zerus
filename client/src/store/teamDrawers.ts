import { invoke } from "@tauri-apps/api/core";
import { inTauri } from "../bridge";
import type { ServiceNote, WaitingItem } from "./teams";

export type DrawerKind = "feed" | "inbox" | "cards";
export type TeamDrawer = DrawerKind | "reports" | "chat" | "timeline";
export interface EffectRecord { request_id: string; node: number; generation: number; member: string; kind: string; state: string; reserved_ms: number; first_outcome_recorded_ms: number | null }
export interface StageRecord { node: number; target: number; generation: number; state: string; created_ms: number }
export interface RecordedFacts { effect: { record: EffectRecord; bookkeeping_elapsed_ms: number | null; clock_or_range_unknown: boolean } | null; stage: StageRecord | null; effect_unavailable: string | null; stage_unavailable: string | null }
export interface TimingSnapshot { schema: number; revision: number }
export interface DrawerRow {
  timing?: RecordedFacts;
  seq: number; kind: string; by: string; text: string; text_truncated: boolean; at_ms: number;
  node: number | null; generation: number | null; state: string; reply: string | null;
  notes: ServiceNote[]; card: WaitingItem | null;
}
export interface RecordedCoverage {
  count: number; limit: number; first_seq: number | null; last_seq: number | null;
  earlier_page: boolean; later_page: boolean; clock_reversed: boolean;
}
export interface DrawerPage {
  timeline?: RecordedCoverage;
  timings?: TimingSnapshot;
  team: string; kind: DrawerKind; rows: DrawerRow[]; after: string | null; next: string | null;
}
export interface DrawerRead { team: string; kind: DrawerKind; after: string | null; recorded_timings?: boolean; timing_seqs?: number[]; timing_next?: string | null }
export type DrawerPort = (read: DrawerRead) => Promise<DrawerPage>;
export const readTeamDrawer: DrawerPort = (read) => {
  if (!inTauri()) return Promise.reject(new Error("Teams need a live hgs event stream."));
  return invoke<DrawerPage>("teams_drawer_page", { read });
};

/** One shown page and at most one queued replacement. No per-team caches or
 * history accumulation. Only the newest scope may publish an async answer. */
export class TeamDrawerRead {
  page: DrawerPage | undefined;
  loading = false;
  error = "";
  private selected: DrawerRead | undefined;
  private pending: { read: DrawerRead; generation: number } | undefined;
  private generation = 0;
  private running = false;
  constructor(private readonly port: DrawerPort, private readonly changed: () => void = () => {}) {}

  open(team: string, kind: DrawerKind) {
    this.page = undefined;
    this.select({ team, kind, after: null });
  }
  timingsRequested() { return !!this.selected?.recorded_timings; }
  timings(on: boolean) {
    if (this.loading || this.selected?.kind !== "feed" || !this.page) return;
    const { recorded_timings: _old, timing_seqs: _seqs, timing_next: _next, ...read } = this.selected;
    if (!on) {
      this.selected = read; this.error = "";
      this.page = { ...this.page, timings: undefined, rows: this.page.rows.map(({ timing: _timing, ...row }) => row) };
      this.changed(); return;
    }
    this.select({ ...read, recorded_timings: true, timing_seqs: this.page.rows.map((row) => row.seq), timing_next: this.page.next });
  }
  next() {
    if (!this.loading && this.page?.next && this.selected) this.select(this.pageRead(this.page.next));
  }
  first() { if (!this.loading && this.selected) this.select(this.pageRead(null)); }
  private pageRead(after: string | null): DrawerRead {
    const { timing_seqs: _seqs, timing_next: _next, ...read } = this.selected!;
    return { ...read, after };
  }
  refresh() { if (this.selected) this.select(this.selected); }
  close() {
    ++this.generation; this.selected = undefined; this.pending = undefined;
    this.page = undefined; this.loading = false; this.error = ""; this.changed();
  }
  private select(read: DrawerRead) {
    this.selected = read; this.error = ""; this.loading = true;
    this.pending = { read, generation: ++this.generation };
    this.changed(); void this.pump();
  }
  private async pump() {
    if (this.running) return;
    this.running = true;
    try {
      while (this.pending) {
        const { read, generation } = this.pending; this.pending = undefined;
        try {
          const page = await this.port(read);
          if (generation !== this.generation) continue;
          if (page.team !== read.team || page.kind !== read.kind || page.after !== read.after || page.rows.length > 200) throw new Error("The team page did not match this drawer.");
          if (!!page.timings !== !!read.recorded_timings || (page.timings && (page.timings.schema !== 1 || !Number.isSafeInteger(page.timings.revision) || page.timings.revision < 1 || page.rows.some((row) => !row.timing)))) throw new Error("The recorded timing page is unsupported. Use Events only.");
          if (read.timing_seqs && (page.rows.length !== read.timing_seqs.length || page.rows.some((row, i) => row.seq !== read.timing_seqs![i]) || page.next !== read.timing_next)) throw new Error("The selected event window changed. Read the page again.");
          this.page = page;
        } catch (error) {
          if (generation !== this.generation) continue;
          this.error = String(error instanceof Error ? error.message : error).slice(0, 2000);
        } finally {
          if (generation === this.generation) { this.loading = false; this.changed(); }
        }
      }
    } finally { this.running = false; }
  }
}
