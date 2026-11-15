/** The whole Settings value from crates/team-plan; never a patch. */
export interface TeamSettings {
  delegations: Record<"accept_results" | "answer_members" | "confirm_trust" | "resume_exited" | "merge_accepted", boolean>;
  caps: Record<"max_members" | "max_phase_tasks" | "max_open_tasks" | "max_open_questions", number>;
  timers: Record<"report_timeout_seconds" | "start_timeout_seconds" | "inbox_interval_seconds" | "remind_lead_minutes" | "remind_person_minutes" | "lead_stall_minutes" | "answer_escalate_minutes" | "resume_limit_per_hour", number>;
  queue: boolean;
  routing: "direct" | "lead";
  plan_approval: "review" | "auto";
  phase_acceptance: "lead" | "person";
  integration: "branch" | "none";
  land_allowed: boolean;
}
