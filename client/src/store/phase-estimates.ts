// The Core's read-only projection. This file holds no preference or cache.
export type EstimateUnknown =
  | "source_identity_unavailable" | "stale_revision" | "generation_unavailable"
  | "history_unavailable" | "unknown_fields" | "incomplete_graph" | "scope_too_large"
  | "incomplete_draft" | "unsettled_publication" | "incomplete_live"
  | "conflicting_membership" | "missing_estimate" | "estimate_over_bound"
  | "external_prerequisite" | "not_implementation";

export interface EstimateVersion {
  phase: number; draft_generation: number; publication: string | null;
}
export interface EstimateScope {
  team: string; home: string; revision: number; version: EstimateVersion;
  basis: "unapproved_preview" | "frozen_publication";
}
export interface EstimateRow {
  node: number; title: string; kind: "task" | "check";
  task: { project: string; uuid: string } | null;
  estimate_minutes: number | null; unknown: EstimateUnknown | null;
}
export interface PhaseEstimatesView {
  /** Estimate membership only; never observation/attempt coverage. */
  scope: EstimateScope | null; cohort_complete: boolean;
  unknown: EstimateUnknown | null; rows: EstimateRow[];
}

export type ComparisonUnknown = "estimate_unavailable" | "outside_cohort" | "not_implementation"
  | "original_publication_unavailable" | "different_publication" | "different_task"
  | "partial_coverage" | "implementation_unsupported";
export interface EstimatePaneView extends PhaseEstimatesView {
  epoch: number;
  window: import("./observed-timing").ObservedPage["scope"]["window"];
  units: { unit: number; node: number; estimate_minutes: number | null;
    coverage: import("./observed-timing").UnitCoverage; unknown: ComparisonUnknown }[];
}
