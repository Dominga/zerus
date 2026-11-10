import { tr, trNoop } from "../i18n";
import type { Catalog, CatalogRole } from "../ui/teams/catalog";
import type { DraftMember, TaskView, TeamSettings } from "../ui/teams/types";

// Task 42: what Qt's TeamNewTab does with the form's fields, pure: the
// template it picks, the members it seats, the folder policy, the lead it
// starts and why Start planning or Create team waits. store/teamsNew.ts keeps
// the draft and its calls; ui/teams/NewTeam.tsx draws this.

/** B6: Auto lets the lead pick per member; Shared is one folder; Worktree one per member. */
export type FolderPolicy = "auto" | "shared" | "worktree";

export const FOLDER_POLICIES: { value: FolderPolicy; label: () => string }[] = [
  { value: "auto", label: () => tr("Auto: the lead picks per member") },
  { value: "shared", label: () => tr("Shared folder") },
  { value: "worktree", label: () => tr("Worktree per member") },
];

export const folderPolicy = (settings: TeamSettings): FolderPolicy => {
  const value = settings.folder_policy;
  return value === "shared" || value === "worktree" ? value : "auto";
};

/** A role's raw field, as the core projects it (CatalogRoleView). */
const field = (role: CatalogRole | undefined, key: string) => {
  const value = role?.raw?.[key];
  return typeof value === "string" ? value : "";
};

/** One new member: of a catalog role, or without a role (Qt addMember). */
export function memberOf(role: CatalogRole | undefined, worktrees: boolean): DraftMember {
  const folder = field(role, "folder");
  return {
    id: "", role: role?.name ?? "", agent: role?.agent || "claude", model: role?.model ?? "", effort: field(role, "effort"),
    account: field(role, "account"), permissions: field(role, "permissions"), computer: field(role, "computer"),
    folder: worktrees && folder !== "read_only" && folder !== "shared" ? "worktree" : "shared", prompt: role?.prompt ?? "",
    lead: false, session: null,
  };
}

/** The roles + Member offers: every catalog role but the lead's. */
export const memberRoles = (catalog: Catalog | undefined) => (catalog?.roles ?? []).filter((role) => role.id !== "lead");

/** Qt nextMemberId: the role as letters, digits and dashes, numbered when taken; "member" without one. */
export function memberId(role: string, taken: ReadonlySet<string>): string {
  let base = role.toLowerCase().replace(/[^a-z0-9]/g, "-").replace(/-+/g, "-").slice(0, 32).replace(/^-+|-+$/g, "");
  if (!base) base = "member";
  let id = base;
  for (let n = 2; taken.has(id); n++) id = `${base}-${n}`;
  return id;
}

/** The members as the draft takes them: ids the draft gave stay; new rows get the next free one for their role. */
export function membersWithIds(members: readonly DraftMember[]): DraftMember[] {
  const taken = new Set<string>(["lead", ...members.map((member) => member.id).filter(Boolean)]);
  return members.map((member) => {
    if (member.id) return { ...member, role: member.role.trim(), model: member.model.trim() };
    const id = memberId(member.role, taken);
    taken.add(id);
    return { ...member, id, role: member.role.trim(), model: member.model.trim() };
  });
}

export interface TemplatePick {
  members: DraftMember[];
  settings: TeamSettings;
  addendum: string;
  planning: string;
}

/**
 * Qt pickTemplate: the template's roster as members, its planning, approval,
 * later-phase approval and limit, and its lead protocol unless the owner wrote
 * one. "-" (No template) seats nobody and keeps the settings.
 */
export function pickTemplate(catalog: Catalog | undefined, id: string, current: { settings: TeamSettings; addendum: string; addendumEdited: boolean }, worktrees: boolean): TemplatePick {
  const template = catalog?.templates.find((found) => found.id === id);
  const members: DraftMember[] = [];
  const settings: TeamSettings = { ...current.settings };
  let planning = "lead";
  if (template) {
    for (const seat of template.roster) {
      const role = catalog!.roles.find((found) => found.id === seat.role);
      for (let i = 0; i < Math.max(1, seat.count); i++) members.push(memberOf(role, worktrees));
    }
    planning = template.planning || "lead";
    settings.approval = template.approval === "auto" ? "auto" : "review";
    const phase = typeof template.raw.phase_approval === "string" ? template.raw.phase_approval : "";
    if (phase) settings.phase_approval = phase; else delete settings.phase_approval;
    if (template.maxMembers > 0) settings.max_members = template.maxMembers;
  }
  const protocol = template && typeof template.raw.lead_protocol === "string" ? template.raw.lead_protocol : "";
  return { members, settings, planning, addendum: current.addendumEdited ? current.addendum : protocol };
}

/** Use template tasks: the template's tasks for the members it seats (Qt templateTasks); none while the lead plans. */
export function templateTasks(catalog: Catalog | undefined, id: string, planning: string, members: readonly DraftMember[]): TaskView[] {
  const template = catalog?.templates.find((found) => found.id === id);
  const tasks = Array.isArray(template?.raw.tasks) ? (template!.raw.tasks as Record<string, unknown>[]) : [];
  if (!template || planning !== "template" || tasks.length === 0) return [];
  const byRole = new Map<string, string>();
  for (const member of membersWithIds(members)) {
    const role = catalog!.roles.find((found) => found.name === member.role);
    if (role && !byRole.has(role.id)) byRole.set(role.id, member.id);
  }
  return tasks.map((task, index) => {
    const role = typeof task.role === "string" ? task.role : "";
    const deps = Array.isArray(task.deps) ? (task.deps as unknown[]).filter((dep): dep is number => typeof dep === "number") : [];
    return {
      id: index + 1, title: typeof task.title === "string" ? task.title : "", status: "pending",
      assignee: role === "lead" ? "lead" : byRole.get(role) ?? "", phase: 1, deps: deps.map((dep) => ({ task: dep + 1, hard: true })),
      estimate_minutes: 0, attempts: 0, attempt_state: "", decided_by: "", pinned: false, output: [], waits: "",
    } as TaskView;
  });
}

/** B6/N10: the policy chosen; a non-Git folder has one folder for everyone, so it is Shared (the select shows it, disabled). */
export function effectivePolicy(chosen: FolderPolicy, folder: string, git: boolean): { policy: FolderPolicy; forced: boolean } {
  if (folder.trim() && !git) return { policy: "shared", forced: true };
  return { policy: chosen, forced: false };
}

/** The members' folders follow the policy: Shared puts everyone in the team folder. */
export function applyPolicy(members: readonly DraftMember[], policy: FolderPolicy, git: boolean): DraftMember[] {
  const worktrees = git && policy !== "shared";
  return members.map((member) => ({ ...member, folder: worktrees ? (member.folder === "read_only" ? "read_only" : member.folder || "worktree") : "shared" }));
}

/** "branch team/<name>/<role>" for a Git folder (no dots as separators). */
export function branchHint(name: string, git: boolean): string {
  if (!git) return "";
  const slug = name.trim().toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "");
  return tr("branch team/%1/<role>", slug || "…");
}

/** Who leads: this session (an existing one) or a new session in the team folder. */
export interface LeadChoice {
  mode: "this" | "new";
  /** The existing session ("this"): its name and agent. */
  session: { name: string; agent: string } | null;
  agent: string; model: string; effort: string; account: string;
}

/** Qt startPlanning's lead: an existing session by name, or a new one with its agent, model, effort and account. */
export function leadMember(choice: LeadChoice, prompt: string): DraftMember {
  const lead: DraftMember = { id: "lead", role: "Lead", agent: choice.agent || "claude", model: "", effort: "", account: "", permissions: "",
    computer: "", folder: "", prompt, lead: true, session: null };
  if (choice.mode === "this" && choice.session) {
    lead.agent = choice.session.agent || choice.session.name.split("/")[0] || "claude";
    lead.session = { host: "", name: choice.session.name, run_id: "", conversation_id: "" };
    return lead;
  }
  return { ...lead, model: choice.model.trim(), effort: choice.effort, account: choice.account };
}

/** Why Start planning waits ("" when it may go). */
export function startReason(folder: string, folderOk: boolean, choice: LeadChoice): string {
  if (!folder.trim() || !folderOk) return tr("Choose an existing folder");
  if (choice.mode === "this" && !choice.session) return tr("Choose who leads");
  return "";
}

/** A member row's problems (Qt rowErrors): no role, an offline computer. */
export type RowProblem = { code: "role" } | { code: "computer-offline"; alias: string };

/** Why Create team waits ("" when it may go), in Qt's order. */
export function createReason(input: {
  drafting: boolean; goal: string; name: string; folderOk: boolean; badRows: number; members: number; max: number; preflightErrors: number;
}): string {
  if (!input.drafting) return tr("Start planning first");
  if (!input.goal.trim()) return tr("Write the goal, or let the lead write it");
  if (!input.name.trim()) return tr("Name the team");
  if (!input.folderOk) return tr("Choose an existing folder");
  if (input.badRows) return input.badRows === 1 ? tr("Fix 1 member row") : tr("Fix %1 member rows", input.badRows);
  if (input.members > input.max) return tr("More members than the limit (the lead is not counted)");
  if (input.preflightErrors) return input.preflightErrors === 1 ? tr("Fix 1 preflight error") : tr("Fix %1 preflight errors", input.preflightErrors);
  return "";
}

/** The limit counts the members besides the lead. */
/**
 * Task 127 (docs/lore.md 4.1): the team's memory modes. With the project's
 * RustCodeLore on, New team offers all five and preselects `project` (a shared
 * workspace in the project's store); with it off, `team` (RustCodeLore in the
 * team's own store), `plain` and `off`, preselecting `team`.
 */
export type MemoryMode = "project" | "team" | "read-only" | "plain" | "off";
export const memoryModes = (projectOn: boolean): MemoryMode[] =>
  projectOn ? ["project", "team", "read-only", "plain", "off"] : ["team", "plain", "off"];
export const defaultMemory = (projectOn: boolean): MemoryMode => (projectOn ? "project" : "team");
/** The mode the settings name, when the project offers it; else the default. Task 126's `lore` is the default. */
export const memoryMode = (settings: TeamSettings, projectOn = false): MemoryMode => {
  const mode = (settings.memory as { mode?: unknown } | undefined)?.mode;
  return memoryModes(projectOn).find((offered) => offered === mode) ?? defaultMemory(projectOn);
};
/** Each mode's label and what it does, as the form shows them. */
export const MEMORY_LABELS: Record<MemoryMode, { label: string; hint: string }> = {
  project: { label: trNoop("Project memory"), hint: trNoop("A shared workspace in the project's RustCodeLore store; the team reads the project's records too") },
  team: { label: trNoop("Team memory"), hint: trNoop("RustCodeLore for this team only: its workspace (in the project's store when it has one, else under Zerus's data)") },
  "read-only": { label: trNoop("Read only"), hint: trNoop("Reads the project's memory and writes nothing") },
  plain: { label: trNoop("Plain memory"), hint: trNoop("The team's own record and a generated digest; no RustCodeLore store") },
  off: { label: trNoop("Off"), hint: trNoop("The team's own record only; no digest and no store") },
};
/** The settings with the chosen memory mode; Zerus fills its store at Create team. */
export const withMemory = (settings: TeamSettings, mode: MemoryMode): TeamSettings => ({ ...settings, memory: { mode } });
export const maxMembers = (settings: TeamSettings) => (typeof settings.max_members === "number" && settings.max_members > 0 ? settings.max_members : 6);
/** Owner, 2026-11-09: the limit counts the members besides the lead (as hgs and the hint say). */
export const canAdd = (members: readonly DraftMember[], settings: TeamSettings) =>
  members.filter((member) => !member.lead && member.id !== "lead").length < maxMembers(settings);
