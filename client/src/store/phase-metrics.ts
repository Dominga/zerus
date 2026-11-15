/** Values from the core's complete-phase projection; the renderer does no
 * graph/estimate arithmetic and never treats zero/missing estimates as zero work. */
export interface PhaseMetricsView {
  unavailable: "incomplete_phase" | "missing_estimates" | "empty_phase" | "invalid_graph" | "scope_too_large" | "unknown_fields" | "live_snapshot_unavailable" | null;
  critical_path_minutes: number | null;
  average_parallelism_tenths: number | null;
  known_estimates: number;
  total_nodes: number;
  external_dependencies: number;
  /** Existing non-fatal engine findings only. The current engine has none. */
  warnings: string[];
}
