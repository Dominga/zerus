// The Teams plan view's inputs. The team, member, task and phase shapes have
// ONE definition in client/src/types.ts, the core's compact view models; this
// module aliases them and adds the plan graph and New team's draft shapes.
import type { TeamMemberView, TeamPhaseView, TeamTaskView } from "../../types";

export type { TeamPipelineRun, TeamView } from "../../types";
export type TaskView = TeamTaskView;
export type PhaseView = TeamPhaseView;
export type MemberView = TeamMemberView;
export type DepView = TeamTaskView["deps"][number];

export interface GraphNode {
  id: number;
  title: string;
  status: string;
  assignee: string;
  role: string;
  phase: number;
  layer: number;
  ready: boolean;
  in_cycle: boolean;
  /** Task #60: the task it was spawned from, when the graph has it (a link, not an edge). */
  origin?: number | null;
}

export interface GraphEdge {
  from: number;
  to: number;
  hard: boolean;
}

export interface PlanGraph {
  team: string;
  nodes: GraphNode[];
  edges: GraphEdge[];
  layers: number;
}

export interface FindingView {
  severity: string;
  code: string;
  text: string;
  task?: number;
  member?: string;
}

export interface ValidationView {
  errors: FindingView[];
  warnings: FindingView[];
}

export interface DraftView {
  revision: number;
  unseen: string[];
  owner_edited: string[];
  /** Task #13 (team ede724c4): the lead's values for the owner's settings, by field. */
  setting_offers?: Record<string, unknown>;
}

export interface DraftMember {
  id: string;
  role: string;
  agent: string;
  model: string;
  effort: string;
  account: string;
  permissions: string;
  computer: string;
  folder: string;
  prompt: string;
  lead: boolean;
  session: SessionView | null;
}

/** Task #308: the working mode hgs proposes for a project. */
export interface WorkingModeProposal {
  preset: string;
  title: string;
  reasons: string[];
  settings?: Record<string, unknown>;
  profile?: Record<string, unknown>;
}

/** Task #308: a running team's mode, its proposal and what the settings miss. */
export interface WorkingModeStatus {
  preset: string;
  proposal?: WorkingModeProposal | null;
  disagreeing?: string[];
}

/** Task #308: a draft's accepted preset (auto by default) and proposal. */
export interface WorkingModeDraft {
  accepted: string;
  preset?: string | null;
  proposal?: WorkingModeProposal | null;
}

export interface NewTeamView {
  team: string;
  name: string;
  goal: string;
  /** Task #308 */
  working_mode?: WorkingModeDraft | null;
  /** Task #8 (team ede724c4): the owner's plan mode, the lead's offer ("" for none) and its reason. */
  plan_mode?: string;
  plan_mode_offer?: string;
  plan_mode_reason?: string;
  addendum: string;
  folder: string;
  settings: Record<string, unknown>;
  members: DraftMember[];
  tasks: TaskView[];
  draft: DraftView;
  validation: ValidationView;
  updated: number;
}

export interface SessionView {
  host: string;
  name: string;
  run_id: string;
  conversation_id: string;
}

export type TeamSettings = Record<string, unknown>;

// Task 47: the team action, call and outcome have one definition, in
// client/src/types.ts, for the one core enum.
export type { TeamAction, TeamCall, TeamConfirmation, TeamOutcome } from "../../types";
