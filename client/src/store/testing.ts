import type { ComputerId, MachineMeta, ProjectedWindow, RowOut, Usage, ViewRow } from "../types";

// Synthetic data for the store and screen tests.

export const NOW = 1_793_000_000;

export function machineMeta(host: string, metrics: Partial<MachineMeta["metrics"]> = {}, extra: Partial<MachineMeta> = {}): MachineMeta {
  return {
    host, hgs_version: "1.56.0", platform: "linux", ok: true, local_sessions: null, local_host: null,
    metrics: {
      cpu_percent: 12, cpu_count: 8, cpu_state: "available", cpu_interval_seconds: 5, load_1: 0.5, load_5: 0.4, load_15: 0.3,
      memory_used_bytes: 4 * 2 ** 30, memory_total_bytes: 16 * 2 ** 30, memory_available_bytes: 12 * 2 ** 30,
      uptime_seconds: 3600, sampled_at: NOW, ...metrics,
    },
    presence: { idle_seconds: null, locked: null, locked_seconds: null },
    ...extra,
  };
}

export const rowOut = (computer: ComputerId, i: number, extra: Partial<ViewRow> = {}): RowOut => ({
  computer, key: `run:${i}`,
  row: { name: `claude/sample-project/t${i}`, agent: "claude", status: "ready", phase: "idle", unread: false,
         machine: computer || "workstation", project: "sample-project", updated_at: NOW + i, ...extra },
});

export const projected = (extra: Partial<ProjectedWindow> = {}): ProjectedWindow =>
  ({ id: "five_hour", label: "", used: 34, minutes: 300, resets_at: NOW + 120 * 60, ...extra });

export const usageOf = (extra: Partial<Usage> = {}): Usage => ({
  status: "ok", provider: "claude", label: "Work", auth_status: "", checked_at: null,
  identity: { email: "", name: "", plan: "", organization: "", account_id: "", auth_method: "" },
  windows: [], balances: [], credits: null, refreshing: false, refresh_error: false, offline: false, seeded: false, ...extra,
});
