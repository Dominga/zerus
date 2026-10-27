// Extra synthetic data for Overview, Projects, New session, worktrees and
// swarm. Every name, path, address and number is invented.
import type { Account, AgentKind } from "./types";

/** Project color presets (ProjectsDialog.cpp:620). Data values, not theme colors. */
export const PROJECT_SWATCHES = [
  "#ffcf33", "#ff9234", "#f85c63", "#e45dbf", "#a96bf5", "#6677ff",
  "#2b97ff", "#23c5dd", "#00aa7f", "#41c86b", "#a6cc38", "#8796aa",
];

/** Launch agents in the dialog's order (NewSessionDialog.cpp:220). */
export const LAUNCH_AGENTS: AgentKind[] = ["codex", "claude", "kimi", "deepseek", "opencode"];

/** Accounts on peers that the shared mock leaves out, plus verified identities. */
export const extraAccounts: (Account & { installed?: boolean })[] = [
  { id: "acc-claude-b1", label: "Aurora engineering", agent: "claude", machine: "build-01", plan: "Max", fiveHour: 34, week: 18, resetIn: "2 h 10 min", sessions: 1, signedIn: true },
  { id: "acc-codex-win", label: "Release review", agent: "codex", machine: "win-lab", plan: "Pro", fiveHour: 43, week: 25, resetIn: "3 h 40 min", sessions: 1, signedIn: true },
  { id: "acc-kimi-mac", label: "Native account", agent: "kimi", machine: "macbook", plan: "", fiveHour: 0, week: 0, resetIn: "-", sessions: 0, signedIn: false, installed: false },
  { id: "acc-claude-b2", label: "Aurora engineering", agent: "claude", machine: "build-02", plan: "Max", fiveHour: 34, week: 18, resetIn: "2 h 10 min", sessions: 1, signedIn: true },
];

/** Verified identity per account profile; equal identities form one Overview card. */
export const accountIdentity: Record<string, { email?: string; name?: string }> = {
  "acc-claude-ws": { email: "eng@example.com" },
  "acc-claude-b1": { email: "eng@example.com" },
  "acc-claude-b2": { email: "eng@example.com" },
  "acc-codex-ws": { email: "eng@example.com" },
  "acc-codex-mac": { email: "release@example.org" },
  "acc-codex-win": { email: "release@example.org" },
  "acc-claude-mac": { email: "release@example.org" },
  "acc-kimi-ws": { name: "Research lab" },
  "acc-ds-b1": { name: "API key" },
  "acc-oc-ws": { name: "Local models" },
};

// ---------------------------------------------------------------------------
// Folder catalogs (WorktreePanel / hgs worktree catalog)

export type CheckoutKind = "main" | "linked" | "bare" | "folder";
export interface CheckoutSession { name: string; state: string; key?: string }
export interface Checkout {
  kind: CheckoutKind;
  path: string;
  branch?: string;
  detached?: string;
  locked?: boolean;
  prunable?: boolean;
  unavailable?: boolean;
  sessions: CheckoutSession[];
}
export type CatalogState = "ok" | "plain" | "missing" | "unavailable" | "offline" | "partial" | "loading";
export interface FolderCatalog {
  state: CatalogState;
  snapshot: string;
  detail?: string;
  checkouts: Checkout[];
  branches: string[];
  cleanable: number;
}

export const catalogs: Record<string, FolderCatalog> = {
  "workstation:/home/example/src/aurora": {
    state: "ok", snapshot: "11:42", cleanable: 2, branches: ["main", "feat/api-v2", "feat/api-v2-cursor", "feat/a11y", "release/2.4", "origin/main"],
    checkouts: [
      { kind: "main", path: "/home/example/src/aurora", branch: "main", sessions: [{ name: "accessibility", state: "Working", key: "ws:4" }, { name: "search-index", state: "Error", key: "ws:5" }, { name: "docs-sync", state: "Waiting", key: "ws:9" }] },
      { kind: "linked", path: "/home/example/src/aurora-work-api-v2", branch: "feat/api-v2", sessions: [{ name: "api-refactor", state: "Working", key: "ws:1" }, { name: "api-refactor-fork", state: "Ready", key: "ws:2" }] },
      { kind: "linked", path: "/home/example/src/aurora-work-cursor", branch: "feat/api-v2-cursor", locked: true, sessions: [{ name: "cursor-bench", state: "Background", key: "ws:3" }] },
      { kind: "linked", path: "/home/example/src/aurora-work-old-pager", branch: "chore/old-pager", sessions: [] },
      { kind: "linked", path: "/home/example/src/aurora-work-3f9a1c20", branch: "work-3f9a1c20", sessions: [{ name: "onboarding-flow", state: "last recorded: Archived" }] },
      { kind: "linked", path: "/home/example/src/aurora-work-77c1d0e4", detached: "77c1d0e4", prunable: true, sessions: [] },
    ],
  },
  "macbook:/Users/example/src/aurora": {
    state: "ok", snapshot: "11:41", cleanable: 1, branches: ["main", "feat/checkout", "origin/main"],
    checkouts: [
      { kind: "main", path: "/Users/example/src/aurora", branch: "main", sessions: [{ name: "release-notes", state: "Ready", key: "mac:2" }] },
      { kind: "linked", path: "/Users/example/src/aurora-work-checkout", branch: "feat/checkout", sessions: [{ name: "checkout-ui", state: "Needs input", key: "mac:1" }] },
      { kind: "linked", path: "/Users/example/src/aurora-work-missing", branch: "work-0b5e93aa", unavailable: true, prunable: true, sessions: [] },
    ],
  },
  "build-01:/srv/example/aurora": {
    state: "partial", snapshot: "11:40", cleanable: 0, branches: ["main", "release/2.4"],
    checkouts: [
      { kind: "main", path: "/srv/example/aurora", branch: "release/2.4", sessions: [{ name: "migrate-db", state: "Needs approval", key: "b1:1" }, { name: "nightly-report", state: "Scheduled", key: "b1:2" }] },
    ],
  },
  "workstation:/home/example/src/infra": {
    state: "ok", snapshot: "11:42", cleanable: 0, branches: ["main", "chore/runner-26"],
    checkouts: [
      { kind: "main", path: "/home/example/src/infra", branch: "chore/runner-26", sessions: [{ name: "runner-images", state: "Working", key: "ws:6" }, { name: "dns-rotation", state: "Scheduled", key: "ws:7" }] },
    ],
  },
  "build-02:/srv/example/runners": {
    state: "offline", snapshot: "11:24", cleanable: 0, branches: ["main"],
    checkouts: [
      { kind: "main", path: "/srv/example/runners", branch: "main", sessions: [{ name: "runner-cleanup", state: "last recorded: Ready", key: "b2:1" }, { name: "disk-audit", state: "last recorded: Working", key: "b2:2" }] },
      { kind: "linked", path: "/srv/example/runners-work-cache", branch: "work-cache-gc", sessions: [] },
    ],
  },
  "macbook:/Users/example/notes/handbook": {
    state: "plain", snapshot: "11:41", cleanable: 0, branches: [],
    checkouts: [{ kind: "folder", path: "/Users/example/notes/handbook", sessions: [{ name: "handbook", state: "Ready", key: "mac:3" }, { name: "paper-notes", state: "Ready", key: "mac:4" }] }],
  },
};

export function catalogFor(machine: string, path: string): FolderCatalog {
  // A linked worktree shares its repository's catalog.
  const shared = Object.entries(catalogs).find(([k, c]) => k.startsWith(`${machine}:`) && c.checkouts.some((x) => x.path === path))?.[1];
  return catalogs[`${machine}:${path}`] ?? shared ?? { state: "plain", snapshot: "11:42", cleanable: 0, branches: [], checkouts: [{ kind: "folder", path, sessions: [] }] };
}

// ---------------------------------------------------------------------------
// Worktrees overview (WorktreesDialog) and batch cleanup

export type Category = 0 | 1 | 2 | 3 | 4;
export type Outcome = "removed" | "forgotten" | "unknown" | "kept" | "programs" | "unsaved";
export interface OverviewRow {
  id: string;
  project: string;
  machine: string;
  path: string;
  branch?: string;
  category: Category;
  state?: string;
  remains: string;
  size: string;
  bytes: number;
  activity: number; // hours since last use
  fresh: boolean;
  integration: string;
  sessions: string[];
  localData?: { path: string; size: string }[];
  discard?: string[];
  processesUnchecked?: boolean;
  outcome: Outcome;
  detail?: string;
}

export const overviewRows: OverviewRow[] = [
  { id: "wt1", project: "aurora", machine: "workstation", path: "/home/example/src/aurora-work-old-pager", branch: "chore/old-pager", category: 0, remains: "Merged into main", size: "412 MB", bytes: 412e6, activity: 220, fresh: true, integration: "Merged into main", sessions: [], outcome: "removed" },
  { id: "wt2", project: "aurora", machine: "workstation", path: "/home/example/src/aurora-work-3f9a1c20", branch: "work-3f9a1c20", category: 0, remains: "Same files as main; original branch kept", size: "1.2 GB", bytes: 1.2e9, activity: 96, fresh: true, integration: "Same files as main; original branch kept", sessions: ["onboarding-flow (archived)"], localData: [{ path: "target/", size: "1.1 GB" }, { path: ".env.local", size: "2 KB" }], outcome: "unknown" },
  { id: "wt3", project: "aurora", machine: "macbook", path: "/Users/example/src/aurora-work-missing", branch: "work-0b5e93aa", category: 0, state: "Missing", remains: "Folder is missing; only Git's record remains", size: "—", bytes: 0, activity: 400, fresh: true, integration: "Integration not verified", sessions: [], outcome: "forgotten" },
  { id: "wt4", project: "infra", machine: "build-02", path: "/srv/example/runners-work-cache", branch: "work-cache-gc", category: 0, state: "Offline snapshot", remains: "Merged into main", size: "88 MB", bytes: 88e6, activity: 300, fresh: false, integration: "Merged into main", sessions: [], outcome: "kept", detail: "Reconnect this computer to check the folder." },
  { id: "wt5", project: "aurora", machine: "macbook", path: "/Users/example/src/aurora-work-checkout", branch: "feat/checkout", category: 1, state: "Not reviewed", remains: "Changes to compare with main, 4 uncommitted files", size: "640 MB", bytes: 640e6, activity: 1, fresh: true, integration: "Changes to compare with main", sessions: ["checkout-ui"], discard: ["web/checkout/Form.tsx", "web/checkout/Form.test.tsx", "web/checkout/strings.nl.json", "notes.txt"], outcome: "unsaved" },
  { id: "wt6", project: "aurora", machine: "workstation", path: "/home/example/src/aurora-work-api-v2", branch: "feat/api-v2", category: 2, remains: "2 sessions use this folder", size: "2.3 GB", bytes: 2.3e9, activity: 0, fresh: true, integration: "Changes to compare with main", sessions: ["api-refactor", "api-refactor-fork"], outcome: "kept", detail: "Another session uses the folder." },
  { id: "wt7", project: "aurora", machine: "workstation", path: "/home/example/src/aurora-work-cursor", branch: "feat/api-v2-cursor", category: 2, state: "Locked", remains: "Locked: a running program uses the folder", size: "980 MB", bytes: 980e6, activity: 0, fresh: true, integration: "Changes to compare with main", sessions: ["cursor-bench"], outcome: "kept", detail: "A running program uses the folder." },
  { id: "wt8", project: "aurora", machine: "build-01", path: "/srv/example/aurora-work-bench", branch: "feat/bench", category: 3, state: "Review unavailable", remains: "Some running programs could not be checked", size: "1.6 GB", bytes: 1.6e9, activity: 30, fresh: true, integration: "Merged into release/2.4", sessions: [], processesUnchecked: true, outcome: "removed" },
  { id: "wt9", project: "aurora", machine: "workstation", path: "/home/example/src/aurora", branch: "main", category: 4, state: "Main checkout", remains: "Main checkout", size: "3.1 GB", bytes: 3.1e9, activity: 0, fresh: true, integration: "", sessions: ["accessibility", "search-index", "docs-sync"], outcome: "kept" },
  { id: "wt10", project: "docs", machine: "macbook", path: "/Users/example/notes/handbook", category: 4, state: "Ordinary folder", remains: "Ordinary folder", size: "24 MB", bytes: 24e6, activity: 0, fresh: true, integration: "", sessions: ["handbook", "paper-notes"], outcome: "kept" },
];

/** Single-repository review rows (WorktreeCleanupDialog) for workstation aurora. */
export type Verdict = "Ready" | "Review" | "Missing" | "Blocked" | "Check running programs";
export interface CleanupRow { path: string; branch: string; verdict: Verdict; details: string; ignored?: string; processes?: boolean; merged?: boolean; result?: "Removed" | "Forgotten" | "Not confirmed" }
export const cleanupRows: CleanupRow[] = [
  { path: "aurora-work-old-pager", branch: "chore/old-pager", verdict: "Ready", details: "Merged into main, used 9 days ago", merged: true, result: "Removed" },
  { path: "aurora-work-3f9a1c20", branch: "work-3f9a1c20", verdict: "Review", details: "Same files as main, 1.1 GB of ignored files", ignored: "1.1 GB", result: "Removed" },
  { path: "aurora-work-77c1d0e4", branch: "detached 77c1d0e4", verdict: "Missing", details: "The folder is gone; only Git metadata remains.", result: "Forgotten" },
  { path: "aurora-work-bench", branch: "feat/bench", verdict: "Check running programs", details: "The OS hides some programs’ folder paths; used 2 hours ago", processes: true, result: "Not confirmed" },
  { path: "aurora-work-api-v2", branch: "feat/api-v2", verdict: "Blocked", details: "2 sessions use this folder (+1 more)" },
];

// ---------------------------------------------------------------------------
// Worktree inspection (WorktreeInspectionPanel)

export const inspection = {
  files: [
    { path: "web/checkout/Form.tsx", state: "M" },
    { path: "web/checkout/Form.test.tsx", state: "M" },
    { path: "web/checkout/strings.nl.json", state: "A" },
    { path: "notes.txt", state: "??" },
  ],
  diff: `--- a/web/checkout/Form.tsx
+++ b/web/checkout/Form.tsx
@@ -12,9 +12,11 @@ export function CheckoutForm() {
-      <Field name="city" />
-      <Field name="street" />
+      <Field name="street" />
+      <Field name="city" />
       <Field name="postal" />
+      <FieldError for="postal" />
       <Field name="country" />`,
  commits: `Original commits absent from the base history (up to 30). Equivalent changes may already be integrated.

9c1e4a7  Reorder checkout fields for keyboard users
41b0d2e  Inline postal code errors
e07f553  Dutch strings for the checkout form`,
  local: [
    { path: "node_modules/", size: "Folder" },
    { path: ".env.local", size: "2 KB" },
    { path: "coverage/", size: "Folder" },
    { path: "dist/", size: "Folder" },
  ],
  preview: `.env.local
API_BASE=https://staging.example.com
FEATURE_CHECKOUT_V2=1`,
};

// ---------------------------------------------------------------------------
// Remote folder browser (DirectoryDialog)

export const remoteDirs: Record<string, string[]> = {
  "/Users/example": [".cache", ".config", "Desktop", "Documents", "notes", "src", "tmp"],
  "/Users/example/src": ["aurora", "aurora-work-checkout", "design-kit", "playground"],
  "/Users/example/notes": ["handbook", "papers"],
  "/Users/example/src/aurora": [".git", "api", "docs", "scripts", "web"],
  "/srv/example": ["aurora", "aurora-work-bench", "datasets", "runners"],
  "/srv/example/datasets": Array.from({ length: 60 }, (_, i) => `shard-${String(i).padStart(4, "0")}`),
  "C:\\src\\example": ["infra", "installer", "tools"],
};

// ---------------------------------------------------------------------------
// OpenCode models (OpenCodeModelPicker)

export interface OcModel { id: string; name: string; provider: string; free?: boolean; isDefault?: boolean; favorite?: boolean; folder?: boolean; unavailable?: boolean; variants?: string[]; short?: boolean }
export const openCodeModels: OcModel[] = [
  { id: "local/qwen3-coder-30b", name: "Qwen3 Coder 30B", provider: "Local", folder: true, free: true, isDefault: true, favorite: true, variants: ["low", "medium", "high"], short: true },
  { id: "local/devstral-small", name: "Devstral Small", provider: "Local", folder: true, free: true, short: true },
  { id: "example-ai/large-2", name: "Example Large 2", provider: "Example AI", favorite: true, variants: ["minimal", "low", "medium", "high", "max"], short: true },
  { id: "example-ai/flash-2", name: "Example Flash 2", provider: "Example AI", variants: ["low", "high"], short: true },
  { id: "example-ai/mini-1", name: "Example Mini 1", provider: "Example AI", free: true },
  { id: "router/coder-pro", name: "Coder Pro", provider: "Model router", variants: ["low", "medium", "high"], short: true },
  { id: "router/coder-lite", name: "Coder Lite", provider: "Model router", free: true },
  { id: "router/reasoner-xl", name: "Reasoner XL", provider: "Model router", variants: ["medium", "high"] },
  { id: "router/vision-2", name: "Vision 2", provider: "Model router" },
  { id: "old/legacy-coder", name: "Legacy Coder", provider: "Unavailable favorites", favorite: true, unavailable: true, short: true },
];

// ---------------------------------------------------------------------------
// Session creation journal (SessionCreationController)

export type CreationState =
  | "saving" | "queued" | "checking" | "checking_repository" | "cloning" | "worktree" | "verifying" | "launching"
  | "confirming" | "stopping" | "stopped" | "recover" | "unknown" | "storage_error" | "failed" | "bound";

export const CREATION_CAPTION: Record<CreationState, string> = {
  saving: "Saving request…", queued: "Queued", checking: "Checking folder…", checking_repository: "Checking repository access…",
  cloning: "Cloning repository…", worktree: "Creating worktree…", verifying: "Verifying checkout…", launching: "Starting agent…",
  confirming: "Confirming session…", stopping: "Stopping setup…", stopped: "Setup stopped", recover: "Continue setup",
  unknown: "Creation result unknown", storage_error: "Could not save creation", failed: "Creation failed", bound: "Session created",
};

export interface CreationRecord {
  id: string;
  tag: string;
  project: string;
  machine: string;
  agent: AgentKind | "shell";
  folder: string;
  worktree?: { path: string; branch: string };
  model?: string;
  state: CreationState;
  lastStage?: CreationState;
  error?: string;
  created: string;
  elapsed: number; // seconds since the last stage update
  startedAt?: number; // epoch ms of the current stage ("Taking longer than usual…")
  createdAt?: number; // epoch ms of the request (m:ss clock)
  endedAt?: number; // epoch ms when the creation settled
  history: { at: string; text: string }[];
  repository?: string;
}

export const seedCreations: CreationRecord[] = [
  {
    id: "c-2", tag: "work-5e1f0c2a", project: "aurora", machine: "build-01", agent: "claude", folder: "/srv/example/aurora", worktree: { path: "/srv/example/aurora-work-5e1f0c2a", branch: "work-5e1f0c2a" },
    state: "unknown", lastStage: "worktree", error: "The computer did not answer before the worktree result was confirmed. Check its original result; the operation will not be repeated.",
    created: "Oct 27 11:31", elapsed: 0, history: [
      { at: "11:31:02", text: "Saving request…" }, { at: "11:31:02", text: "Queued" }, { at: "11:31:03", text: "Checking folder…" },
      { at: "11:31:05", text: "Creating worktree…" }, { at: "11:32:05", text: "Creation result unknown" },
    ],
  },
  {
    id: "c-3", tag: "docs-refresh", project: "docs", machine: "macbook", agent: "codex", folder: "/Users/example/notes/handbook",
    state: "failed", lastStage: "checking", error: "The computer did not confirm this folder. Try again.", created: "Oct 27 10:58", elapsed: 0,
    history: [{ at: "10:58:40", text: "Saving request…" }, { at: "10:58:40", text: "Queued" }, { at: "10:58:41", text: "Checking folder…" }, { at: "10:58:44", text: "Creation failed" }],
  },
  {
    id: "c-4", tag: "bench-arm", project: "infra", machine: "workstation", agent: "codex", folder: "/home/example/src/infra", repository: "git@example.org:team/bench.git",
    state: "stopped", lastStage: "cloning", error: "Preparation ended. The checkout and branch are kept.", created: "Oct 27 09:12", elapsed: 0,
    history: [{ at: "09:12:10", text: "Saving request…" }, { at: "09:12:11", text: "Checking repository access…" }, { at: "09:12:13", text: "Cloning repository…" }, { at: "09:13:02", text: "Stopping setup…" }, { at: "09:13:20", text: "Setup stopped" }],
  },
];

// ---------------------------------------------------------------------------
// Project swarm (SwarmDialog, SwarmConflictReview)

export const swarmComputers = [
  { computer: "workstation", access: "This computer", sync: "Local catalog" },
  { computer: "macbook", access: "macbook", sync: "Synced 11:42:08" },
  { computer: "build-01", access: "build-01", sync: "Synced 11:42:01. Update Zerus there to share tasks" },
  { computer: "build-02", access: "build-02", sync: "SSH request timed out" },
  { computer: "studio", access: "Not configured", sync: "Shared through swarm" },
];

export const swarmPeerCatalog: Record<string, { same: boolean; projects: string[] }> = {
  "macbook": { same: true, projects: [] },
  "build-01": { same: true, projects: [] },
  "build-02": { same: true, projects: [] },
  "win-lab": { same: false, projects: ["Infra (Windows)", "Installer", "Aurora platform", "General"] },
};

export interface ConflictChoice { value: string; machine: string; local?: boolean; shown?: boolean; removal?: boolean }
export interface Conflict { id: string; kind: string; subject: string; machine?: string; explanation: string; choices: ConflictChoice[]; action: string; outcome: (c: ConflictChoice) => string }

export const swarmConflicts: Conflict[] = [
  {
    id: "k1", kind: "Session project", subject: "checkout-ui on macbook", machine: "macbook",
    explanation: "Connected computers have different project assignments for this session. Choose the project it should belong to.\n“Shown now” is a temporary display choice, not a resolved conflict.",
    choices: [{ value: "Aurora platform", machine: "workstation", local: true, shown: true }, { value: "Docs & research", machine: "macbook" }],
    action: "Keep this project",
    outcome: (c) => `Assign this session to “${c.value}”. The session, its messages and running agent stay intact.\nThis choice will synchronize to connected computers. Offline computers receive it when they reconnect.`,
  },
  {
    id: "k2", kind: "Project folder", subject: "Runners on build-02", machine: "build-02",
    explanation: "Different folder locations or a removal were saved before the changes were synchronized. Choose the folder link to keep.\n“Shown now” is a temporary display choice, not a resolved conflict.",
    choices: [{ value: "Name: Runners\nProject: Infrastructure\nComputer: build-02\n/srv/example/runners", machine: "workstation", local: true, shown: true }, { value: "Remove folder link from the catalog", machine: "win-lab", removal: true }],
    action: "Keep this folder link",
    outcome: (c) => (c.removal ? "Remove this folder link from the shared catalog." : `Keep this folder location:\n${c.value}`) + "\nFiles on disk stay intact.\nThis choice will synchronize to connected computers. Offline computers receive it when they reconnect.",
  },
  {
    id: "k3", kind: "Keep or remove project", subject: "Docs & research",
    explanation: "Different changes to this setting were saved before they were synchronized. Choose the value to keep.\n“Shown now” is a temporary display choice, not a resolved conflict.",
    choices: [{ value: "Keep project in the catalog", machine: "workstation", local: true, shown: true }, { value: "Remove project from the catalog", machine: "macbook", removal: true }],
    action: "Apply project choice",
    outcome: (c) => (c.removal ? "Remove “Docs & research” from the shared catalog. Its folder links and project placement will no longer be displayed. Files, sessions and messages stay intact." : "Keep “Docs & research” in the shared catalog.") + "\nThis choice will synchronize to connected computers. Offline computers receive it when they reconnect.",
  },
];
