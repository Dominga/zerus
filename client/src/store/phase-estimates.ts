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
  scope: EstimateScope | null; cohort_complete: boolean;
  unknown: EstimateUnknown | null; rows: EstimateRow[];
}
