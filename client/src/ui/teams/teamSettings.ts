import type { TeamMemberView } from "../../types";

export interface SettingsForm {
  ironRules: string;
  branchOn: boolean;
  template: string;
  base: string;
  restart: boolean;
  land: boolean;
  broker: boolean;
  leadUnpause: boolean;
  acceptance: boolean;
}

export function lines(value: unknown): string[] {
  return Array.isArray(value) ? value.filter((line): line is string => typeof line === "string") : [];
}

/** The settings patch Qt's changes() builds: only fields that differ, and
 * branch_per_task is an object, or null when switched off. */
export function settingsPatch(settings: Record<string, unknown>, form: SettingsForm): Record<string, unknown> {
  const patch: Record<string, unknown> = {};
  if (form.ironRules.split("\n").map((line) => line.trim()).filter(Boolean).join("\n") !== lines(settings.iron_rules).join("\n")) {
    patch.iron_rules = form.ironRules.split("\n").map((line) => line.trim()).filter(Boolean);
  }
  const branch = settings.branch_per_task as { template?: string } | null | undefined;
  const before = branch?.template || "";
  const after = form.branchOn ? form.template.trim() : "";
  if (after !== before) patch.branch_per_task = after ? { template: after } : null;
  if (form.base.trim() !== String(settings.base_branch ?? "")) patch.base_branch = form.base.trim();
  if (form.restart !== Boolean(settings.restart_crashed)) patch.restart_crashed = form.restart;
  if (form.land !== Boolean(settings.land_allowed)) patch.land_allowed = form.land;
  if (form.broker !== Boolean(settings.build_broker)) patch.build_broker = form.broker;
  if (form.leadUnpause !== Boolean(settings.lead_removes_own_pause)) patch.lead_removes_own_pause = form.leadUnpause;
  if (form.acceptance !== (settings.phase_acceptance === "owner")) {
    patch.phase_acceptance = form.acceptance ? "owner" : "lead";
  }
  return patch;
}

/** The members_update entries for members whose iron rules changed. */
export function memberIronUpdates(
  members: TeamMemberView[],
  rules: Record<string, string>,
): { id: string; iron_rules: string[] }[] {
  return members
    .filter((member) => member.state !== "removed")
    .filter((member) => (rules[member.id] ?? "") !== lines(member.iron_rules).join("\n"))
    .map((member) => ({
      id: member.id,
      iron_rules: (rules[member.id] ?? "").split("\n").map((line) => line.trim()).filter(Boolean),
    }));
}
