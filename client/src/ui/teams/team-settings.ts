import type { TeamSettings } from "../../store/team-settings";

export type { TeamSettings } from "../../store/team-settings";
export const DELEGATIONS = ["accept_results", "answer_members", "confirm_trust", "resume_exited", "merge_accepted"] as const;
export const CAPS = ["max_members", "max_phase_tasks", "max_open_tasks", "max_open_questions"] as const;
export const TIMERS = ["report_timeout_seconds", "start_timeout_seconds", "inbox_interval_seconds", "remind_lead_minutes", "remind_person_minutes", "lead_stall_minutes", "answer_escalate_minutes", "resume_limit_per_hour"] as const;
export const capMax = (key: typeof CAPS[number]) => key === "max_open_tasks" ? 500 : 200;

const object = (value: unknown): value is Record<string, unknown> => !!value && typeof value === "object" && !Array.isArray(value);
const exact = (value: unknown, keys: readonly string[]): value is Record<string, unknown> => object(value) && Object.keys(value).length === keys.length && keys.every((key) => Object.hasOwn(value, key));

/** An editor must never fill missing fields with defaults or discard future ones. */
export function completeSettings(value: unknown): value is TeamSettings {
  if (!exact(value, ["delegations", "caps", "timers", "queue", "routing", "plan_approval", "phase_acceptance", "integration", "land_allowed"])) return false;
  const delegations = value.delegations, caps = value.caps, timers = value.timers;
  const choice = (entry: unknown, options: string[]) => typeof entry === "string" && options.includes(entry);
  return exact(delegations, DELEGATIONS) && DELEGATIONS.every((key) => typeof delegations[key] === "boolean")
    && exact(caps, CAPS) && CAPS.every((key) => Number.isInteger(caps[key]) && Number(caps[key]) >= 1 && Number(caps[key]) <= capMax(key))
    && exact(timers, TIMERS) && TIMERS.every((key) => Number.isInteger(timers[key]) && Number(timers[key]) >= 0 && Number(timers[key]) <= 0xffffffff)
    && typeof value.queue === "boolean" && typeof value.land_allowed === "boolean"
    && choice(value.routing, ["direct", "lead"]) && choice(value.plan_approval, ["review", "auto"])
    && choice(value.phase_acceptance, ["lead", "person"]) && choice(value.integration, ["branch", "none"]);
}

/** HTML number inputs may be blank, fractional or outside u32: none becomes zero. */
export function wholeNumber(text: string, min: number, max: number): number | undefined {
  if (!/^\d+$/.test(text)) return undefined;
  const number = Number(text);
  return Number.isSafeInteger(number) && number >= min && number <= max ? number : undefined;
}
