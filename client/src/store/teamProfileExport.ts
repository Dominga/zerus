import { invoke } from "@tauri-apps/api/core";
import { inTauri } from "../bridge";
import type { DrawerPage } from "./teamDrawers";

export function recordedSnapshot(page: DrawerPage) {
  return structuredClone({
    schema: 1, kind: "recorded_team_events", source: "local_hgs_displayed_page", scope: "journal_page",
    computer: null, phase: null, history_complete: false,
    team: page.team, after: page.after, next: page.next, timeline: page.timeline,
    timings: page.timings ?? null,
    rows: page.rows.map(({ seq, at_ms, kind, by, text, text_truncated, timing }) => ({ seq, at_ms, kind, by, text, text_truncated, timing: timing ?? null })),
  });
}
export type PageExport = ReturnType<typeof recordedSnapshot>;
export type PageExportPort = (value: PageExport) => Promise<boolean>;
export const exportRecordedPage: PageExportPort = (value) => {
  if (!inTauri()) return Promise.reject(new Error("Page export needs the desktop save dialog."));
  return invoke<boolean>("team_profile_export", { value });
};

/** One selected immutable payload. An unknown result never schedules a repost. */
export class RecordedPageExport {
  busy = false;
  outcome: "none" | "saved" | "cancelled" | "unknown" = "none";
  error = "";
  private generation = 0;
  constructor(private readonly port: PageExportPort, private readonly changed: () => void = () => {}) {}
  retire() { ++this.generation; this.outcome = "none"; this.error = ""; this.changed(); }
  async save(page: DrawerPage) {
    if (this.busy || !page.timeline) return;
    const generation = ++this.generation;
    this.busy = true; this.outcome = "none"; this.error = ""; this.changed();
    try {
      const value = recordedSnapshot(page);
      const saved = await this.port(value);
      if (typeof saved !== "boolean") throw new Error("The export result was not understood.");
      if (generation === this.generation) this.outcome = saved ? "saved" : "cancelled";
    } catch (error) {
      if (generation === this.generation) { this.outcome = "unknown"; this.error = String(error instanceof Error ? error.message : error).slice(0, 2000); }
    } finally { this.busy = false; this.changed(); }
  }
}
