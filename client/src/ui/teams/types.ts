// The Teams plan view's inputs: the compact view models the core projects
// (client/core/src/teams/view.rs) and the plan graph. The webview parses no
// `hgs team` reply; these are already bounded and sanitized.

export interface DepView {
  task: number;
  hard: boolean;
}

export interface TeamTaskStage {
  kind: string;
  state: string;
  attempt: number;
  member: string;
}

export interface TaskPipelineView {
  name: string;
  round: number;
  stages: TeamTaskStage[];
}

export interface TaskView {
  id: number;
  title: string;
  status: string;
  assignee: string;
  phase: number;
  deps: DepView[];
  estimate_minutes: number;
  attempts: number;
  attempt_state: string;
  decided_by: string;
  pinned: boolean;
  output: string[];
  waits: string;
  pipeline?: TaskPipelineView | null;
}

export interface PhaseView {
  n: number;
  title: string;
  state: string;
  status: string;
  done: number;
  total: number;
  barrier: boolean;
  auto: boolean;
  acceptance: string;
  paused: boolean;
  health: string;
}

export interface MemberView {
  id: string;
  role: string;
  agent: string;
  model: string;
  effort: string;
  account: string;
  computer: string;
  folder: string;
  state: string;
  activity: string;
  phase: string;
  error: string;
  lead: boolean;
  queue: number[];
  live: boolean | null;
  stuck: boolean;
  question: boolean;
  host_shared: boolean;
  minutes_without_progress: number;
}

export interface TeamView {
  id: string;
  name: string;
  status: string;
  project: string;
  folder: string;
  goal: string;
  plan_mode: string;
  lead: string;
  revision: number;
  created: number;
  updated: number;
  waiting_for_lead: boolean;
  members: MemberView[];
  tasks: TaskView[];
  phases: PhaseView[];
  ready: number[];
  cycle: number[];
  truncated: boolean;
}

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

export interface NewTeamView {
  team: string;
  name: string;
  goal: string;
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

