import { tr } from "../../../i18n";
import type { TeamAction, TeamOutcome } from "../../../types";
import type { DecisionPort } from "../../teams/decisions";

// Task 108: Settings > Build folders. What Zerus holds on this computer per
// team and folder (`hgs team disk`), the cleanup policy, a dry run and Clean
// now. Clean now goes through the core's confirmation (Cancel is focused);
// only Zerus's own build output is ever removed, and a folder in use is kept.

export interface DiskPolicy { enabled: boolean; unused_days: number; free_gb: number; free_percent: number }
export interface DiskDrive { drive: string; free: number; total: number; threshold: number; low: boolean }
export interface DiskFolder {
  path: string; kind: string; origin: string; owned: boolean; owned_reason: string; state: string; note?: string;
  size?: number; size_complete?: boolean; last_used: number; due?: string | null; exists?: boolean; member?: string;
}
export interface DiskTeam { team: string; team_name: string; bytes: number; folders: DiskFolder[] }
export interface DiskView { policy: DiskPolicy; drives: DiskDrive[]; teams: DiskTeam[]; held: number; last_pass: number }
export interface DiskDecision { path: string; team_name: string; reason: string; action: "remove" | "keep" | "review"; detail: string; bytes: number }

const GB = 1024 * 1024 * 1024;

/** "12.3 GB"; a smaller size in MB. */
export function size(bytes: number | undefined): string {
  if (bytes === undefined) return tr("not measured yet");
  if (bytes >= GB / 10) return tr("%1 GB", (bytes / GB).toFixed(1));
  return tr("%1 MB", Math.max(0, bytes / (1024 * 1024)).toFixed(0));
}

/** "D: 12.3 GB free of 931.5 GB, keeps 93.2 GB" (never dots between parts). */
export function driveLine(drive: DiskDrive): string {
  const line = tr("%1 %2 free of %3, keeps %4", drive.drive, size(drive.free), size(drive.total), size(drive.threshold));
  return drive.low ? tr("%1; low on space", line) : line;
}

/** What one folder's row says about it. */
export function folderState(folder: DiskFolder): string {
  if (folder.state === "review") return tr("Waits for your review: %1", folder.note ?? "");
  if (!folder.owned) return tr("Kept: not Zerus's (%1)", folder.owned_reason);
  if (folder.exists === false) return tr("Gone");
  if (folder.due) return tr("The policy removes it: %1", folder.due);
  return tr("Kept");
}

/** "Would free 31.2 GB in 4 folders; 2 kept" for a dry run, or what Clean now did. */
export function decisionsLine(decisions: readonly DiskDecision[], dry: boolean): string {
  const removed = decisions.filter((decision) => decision.action === "remove");
  const kept = decisions.length - removed.length;
  const bytes = removed.reduce((sum, decision) => sum + decision.bytes, 0);
  if (decisions.length === 0) return tr("Nothing to remove now");
  const head = dry ? tr("Would free %1 in %2 folders", size(bytes), removed.length) : tr("Freed %1 in %2 folders", size(bytes), removed.length);
  return kept > 0 ? tr("%1; %2 kept", head, kept) : head;
}

/** Reads and changes the Disk view through the core's team actions. */
export class DiskModel {
  view: DiskView | undefined;
  decisions: DiskDecision[] = [];
  dry = true;
  notice = "";
  busy = false;
  constructor(private port: DecisionPort, private changed: () => void) {}

  private async send(action: TeamAction): Promise<Record<string, unknown> | undefined> {
    this.busy = true;
    this.notice = "";
    this.changed();
    try {
      const call = { computer: "", team: "", team_name: "", action };
      let outcome: TeamOutcome = await this.port.send(call);
      if (outcome.outcome === "confirm") {
        if (!(await this.port.confirm(outcome.confirmation))) return undefined;
        outcome = await this.port.send({ ...call, confirmed: true });
      }
      if (outcome.outcome === "done") return (outcome.receipt ?? {}) as Record<string, unknown>;
      this.notice = outcome.outcome === "uncertain" ? tr("It may have run: %1", outcome.detail) : "detail" in outcome ? outcome.detail : "";
      return undefined;
    } finally {
      this.busy = false;
      this.changed();
    }
  }

  async load(refresh = false) {
    const receipt = await this.send({ kind: "disk", action: "list", ...(refresh ? { refresh: true } : {}) });
    if (receipt) this.view = receipt as unknown as DiskView;
    this.changed();
  }

  /** A dry run, or Clean now (the core asks the owner first). */
  async clean(dry: boolean, scope: "policy" | "all" = "policy") {
    const receipt = await this.send({ kind: "disk", action: "clean", scope, dry_run: dry });
    if (!receipt) return;
    this.dry = dry;
    this.decisions = (receipt.decisions ?? []) as DiskDecision[];
    if (!dry) await this.load();
    this.changed();
  }

  async savePolicy(policy: DiskPolicy) {
    const receipt = await this.send({ kind: "disk", action: "policy", policy });
    if (receipt) await this.load();
  }

  async forget(path: string) {
    if (await this.send({ kind: "disk", action: "forget", path })) await this.load();
  }
}
