import { createSignal } from "solid-js";
import type {
  DraftMember,
  NewTeamView,
  TaskView,
  TeamCall,
  TeamOutcome,
  TeamSettings,
} from "../ui/teams/types";

export const DRAFT_FIELDS = ["name", "goal", "addendum", "settings", "members", "tasks"] as const;
export type DraftField = (typeof DRAFT_FIELDS)[number];

export interface NewTeamFields {
  name: string;
  goal: string;
  addendum: string;
  folder: string;
  settings: TeamSettings;
  members: DraftMember[];
  tasks: TaskView[];
}

export const emptyFields = (): NewTeamFields => ({
  name: "",
  goal: "",
  addendum: "",
  folder: "",
  settings: {},
  members: [],
  tasks: [],
});

export interface NewTeamPort {
  action(call: TeamCall): Promise<TeamOutcome>;
}

export interface ComputerChoice {
  alias: string;
  label: string;
  online: boolean;
}

export type RowError = { code: "role" } | { code: "computer-offline"; alias: string };

/** A member row's live error: no role, or a computer that is not online. */
export function memberRowErrors(member: DraftMember, computers: ComputerChoice[]): RowError[] {
  const errors: RowError[] = [];
  if (!member.role.trim()) errors.push({ code: "role" });
  if (member.computer) {
    const computer = computers.find((candidate) => candidate.alias === member.computer);
    if (!computer || !computer.online) errors.push({ code: "computer-offline", alias: member.computer });
  }
  return errors;
}

/** Only the settings that differ, as `hgs team settings` merges them. */
export function settingsChanges(before: TeamSettings, after: TeamSettings): TeamSettings {
  const changes: TeamSettings = {};
  for (const key of new Set([...Object.keys(before), ...Object.keys(after)])) {
    if (JSON.stringify(before[key]) !== JSON.stringify(after[key])) {
      changes[key] = after[key] ?? null;
    }
  }
  return changes;
}

/**
 * A polled draft merged into the open form: every field the owner has not
 * edited follows the lead; a field the owner edited keeps the owner's value.
 * The lead's version still arrives in `view.draft.unseen`, so the form can
 * mark it, but it never overwrites unsaved typing.
 */
export function mergeDraft(
  local: NewTeamFields,
  dirty: ReadonlySet<DraftField>,
  view: NewTeamView,
): NewTeamFields {
  const merged: NewTeamFields = { ...local };
  for (const field of DRAFT_FIELDS) {
    if (!dirty.has(field)) {
      (merged as unknown as Record<string, unknown>)[field] = view[field as keyof NewTeamView];
    }
  }
  merged.folder = view.folder || local.folder;
  return merged;
}

export interface StartPlanningInput {
  computer?: string;
  folder: string;
  name?: string;
  lead: DraftMember;
  project?: string;
  project_id?: string;
  template?: string;
}

export type NewTeamResult = { ok: true; outcome: TeamOutcome } | { ok: false; error: string };

export function createNewTeam(port: NewTeamPort) {
  const [open, setOpen] = createSignal(false);
  const [view, setView] = createSignal<NewTeamView | null>(null);
  const [fields, setFields] = createSignal<NewTeamFields>(emptyFields());
  const [dirty, setDirty] = createSignal<Set<DraftField>>(new Set());
  const [unseen, setUnseen] = createSignal<string[]>([]);
  const [draftId, setDraftId] = createSignal("");
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal("");
  const [created, setCreated] = createSignal("");

  const openNew = () => {
    // Opening the tab creates nothing: no draft, no session, no call.
    setOpen(true);
    setView(null);
    setFields(emptyFields());
    setDirty(new Set<DraftField>());
    setUnseen([]);
    setDraftId("");
    setCreated("");
    setError("");
  };
  const close = () => setOpen(false);

  const edit = (field: DraftField, value: NewTeamFields[DraftField]) => {
    setFields({ ...fields(), [field]: value } as NewTeamFields);
    setDirty(new Set<DraftField>([...dirty(), field]));
  };
  const setFolder = (value: string) => setFields({ ...fields(), folder: value });

  const applyView = (next: NewTeamView) => {
    setFields(mergeDraft(fields(), dirty(), next));
    setView(next);
    setUnseen(next.draft.unseen);
    setDraftId(next.team);
  };

  const startPlanning = async (input: StartPlanningInput): Promise<NewTeamResult> => {
    if (!input.folder.trim()) return { ok: false, error: "folder" };
    if ((input.name ?? "").trim().length > 80) return { ok: false, error: "name" };
    setBusy(true);
    setError("");
    try {
      const outcome = await port.action({
        computer: input.computer ?? "",
        team: "",
        team_name: input.name ?? "",
        confirmed: false,
        action: {
          kind: "draft_start",
          folder: input.folder,
          name: input.name,
          lead: input.lead,
          project: input.project,
          project_id: input.project_id,
          template: input.template,
        },
      });
      if (outcome.outcome !== "done") {
        setError(outcome.outcome === "confirm" ? outcome.confirmation.text : outcome.detail);
        return { ok: false, error: "start" };
      }
      const team = String(outcome.receipt.team ?? "");
      setDraftId(team);
      setFields({ ...fields(), folder: input.folder, name: input.name ?? "", members: [input.lead] });
      return { ok: true, outcome };
    } catch (cause) {
      setError(String(cause));
      return { ok: false, error: "start" };
    } finally {
      setBusy(false);
    }
  };

  const createTeam = async (): Promise<NewTeamResult> => {
    const current = view();
    if (!current || !draftId()) return { ok: false, error: "no-draft" };
    setBusy(true);
    setError("");
    try {
      if (dirty().size > 0) {
        const set: Record<string, unknown> = {};
        for (const field of dirty()) set[field] = fields()[field];
        const update = await port.action({
          team: draftId(),
          team_name: fields().name,
          confirmed: false,
          action: { kind: "draft_update", revision: current.draft.revision, set },
        });
        if (update.outcome !== "done") {
          setError(update.outcome === "confirm" ? update.confirmation.text : update.detail);
          return { ok: false, error: "update" };
        }
        setDirty(new Set<DraftField>());
      }
      const outcome = await port.action({
        team: draftId(),
        team_name: fields().name,
        confirmed: false,
        action: { kind: "draft_create", approve: false, confirm_widening: false },
      });
      if (outcome.outcome !== "done") {
        setError(outcome.outcome === "confirm" ? outcome.confirmation.text : outcome.detail);
        return { ok: false, error: "create" };
      }
      setCreated(draftId());
      return { ok: true, outcome };
    } catch (cause) {
      setError(String(cause));
      return { ok: false, error: "create" };
    } finally {
      setBusy(false);
    }
  };

  const saveSettings = async (patch: TeamSettings): Promise<NewTeamResult> => {
    const current = view();
    if (!current) return { ok: false, error: "no-team" };
    const changes = settingsChanges((current.settings as TeamSettings) ?? {}, patch);
    if (Object.keys(changes).length === 0) return { ok: true, outcome: { outcome: "done", receipt: {}, request_id: "" } };
    setBusy(true);
    setError("");
    try {
      const outcome = await port.action({
        team: current.team,
        team_name: current.name,
        confirmed: false,
        action: { kind: "settings", settings: changes },
      });
      if (outcome.outcome !== "done") {
        setError(outcome.outcome === "confirm" ? outcome.confirmation.text : outcome.detail);
        return { ok: false, error: "settings" };
      }
      return { ok: true, outcome };
    } catch (cause) {
      setError(String(cause));
      return { ok: false, error: "settings" };
    } finally {
      setBusy(false);
    }
  };

  const markSeen = async (field: string) => {
    if (!draftId() || !unseen().includes(field)) return;
    setUnseen(unseen().filter((value) => value !== field));
    await port.action({
      team: draftId(),
      team_name: fields().name,
      confirmed: false,
      action: { kind: "draft_seen", fields: [field] },
    });
  };

  return {
    open,
    openNew,
    close,
    view,
    fields,
    dirty,
    unseen,
    draftId,
    busy,
    error,
    created,
    edit,
    setFolder,
    applyView,
    startPlanning,
    createTeam,
    saveSettings,
    markSeen,
  };
}

export type NewTeamStore = ReturnType<typeof createNewTeam>;
