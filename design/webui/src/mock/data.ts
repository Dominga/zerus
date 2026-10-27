// Synthetic fleet for the design mockup. Every name, path, domain and number
// here is invented; no real session content is used.
import type { Account, ActivityItem, Machine, Project, Session, Task } from "./types";

export const machines: Machine[] = [
  { id: "m-ws", alias: "workstation", label: "This computer", platform: "linux", connection: "online", thisComputer: true, colorIndex: 1, cpu: 24, cores: 16, load: 2.1, memUsedGiB: 9.0, memTotalGiB: 32, hgsVersion: "0.48.0-dev.912", lastSeen: "now", latencyMs: 0 },
  { id: "m-mac", alias: "macbook", label: "macbook", platform: "macos", connection: "online", thisComputer: false, colorIndex: 2, cpu: 36, cores: 10, load: 3.1, memUsedGiB: 10.0, memTotalGiB: 32, hgsVersion: "0.48.0-dev.912", lastSeen: "now", latencyMs: 14 },
  { id: "m-b1", alias: "build-01", label: "build-01", platform: "linux", connection: "online", thisComputer: false, colorIndex: 0, cpu: 48, cores: 32, load: 4.1, memUsedGiB: 11.0, memTotalGiB: 64, hgsVersion: "0.48.0-dev.909", lastSeen: "now", latencyMs: 22 },
  { id: "m-b2", alias: "build-02", label: "build-02", platform: "linux", connection: "offline", thisComputer: false, colorIndex: 3, cpu: 0, cores: 32, load: 0, memUsedGiB: 0, memTotalGiB: 64, hgsVersion: "0.47.2", lastSeen: "18 min ago", latencyMs: 0 },
  { id: "m-win", alias: "win-lab", label: "win-lab", platform: "windows", connection: "reconnecting", thisComputer: false, colorIndex: 5, cpu: 12, cores: 8, load: 0.6, memUsedGiB: 6.2, memTotalGiB: 16, hgsVersion: "0.48.0-dev.912", lastSeen: "40 s ago", latencyMs: 0 },
];

export const projects: Project[] = [
  { id: "aurora", name: "Aurora platform", color: "#00ab80", colorIndex: 0, vivid: false, isDefault: false, folders: [
    { name: "API and web app", machine: "workstation", path: "/home/example/src/aurora" },
    { name: "macOS client", machine: "macbook", path: "/Users/example/src/aurora" },
    { name: "Integration tests", machine: "build-01", path: "/srv/example/aurora" },
  ] },
  { id: "infra", name: "Infrastructure", colorIndex: 3, vivid: false, isDefault: false, folders: [
    { name: "Deploy scripts", machine: "workstation", path: "/home/example/src/infra" },
    { name: "Runners", machine: "build-02", path: "/srv/example/runners" },
  ] },
  { id: "docs", name: "Docs & research", colorIndex: 2, vivid: false, isDefault: false, folders: [
    { name: "Handbook", machine: "macbook", path: "/Users/example/notes/handbook" },
  ] },
  { id: "general", name: "General", colorIndex: 7, vivid: false, isDefault: true, folders: [] },
];

const S = (s: Partial<Session> & Pick<Session, "key" | "name" | "machine" | "project" | "agent" | "status" | "summary">): Session => ({
  model: "claude-opus-4-6",
  effort: "high",
  cwd: "/home/example/src/aurora",
  branch: "main",
  lastEvent: "2 min ago",
  unread: false,
  draft: false,
  account: "acc-claude-ws",
  ...s,
});

export const sessions: Session[] = [
  // Aurora platform
  S({ key: "ws:1", name: "api-refactor", machine: "workstation", project: "aurora", agent: "claude", status: "working", phase: "tool", summary: "Bash: cargo test --workspace", tool: "Bash", turnStartedMin: 4, branch: "feat/api-v2", worktree: "api-v2", git: { ahead: 3, behind: 0, changed: 7, state: "diff" }, subagents: [
    { id: "sa1", name: "API review", state: "working", summary: "Check pagination and response shapes" },
    { id: "sa2", name: "macOS parity", state: "working", summary: "Verify native client behavior" },
    { id: "sa3", name: "Regression tests", state: "ready", summary: "42 checks passed" },
    { id: "sa4", name: "Migration notes", state: "ready", summary: "Documented the rollout" },
  ], contextPct: 36, tag: "v2", goal: "Ship the paginated v2 API without breaking v1 clients" }),
  S({ key: "ws:2", name: "api-refactor-fork", machine: "workstation", project: "aurora", agent: "claude", status: "ready", summary: "Compared both pagination strategies", unread: true, attention: "reply", branch: "feat/api-v2-cursor", git: { ahead: 1, behind: 0, changed: 0, state: "push" } }),
  S({ git: { ahead: 0, behind: 0, changed: 0, state: "synced" }, key: "ws:3", name: "cursor-bench", machine: "workstation", project: "aurora", agent: "codex", model: "gpt-6.1-sol", status: "background", summary: "Idle; benchmark still running", backgroundJobs: 1, backgroundCommand: "cargo bench -p pager", account: "acc-codex-ws", branch: "feat/api-v2-cursor" }),
  S({ key: "mac:1", name: "checkout-ui", machine: "macbook", project: "aurora", agent: "codex", model: "gpt-6.1-sol", status: "attention", attention: "question", summary: "Which form layout should I keep?", cwd: "/Users/example/src/aurora", branch: "feat/checkout", account: "acc-codex-mac", git: { ahead: 2, behind: 1, changed: 4, state: "diff" } }),
  S({ key: "b1:1", name: "migrate-db", machine: "build-01", project: "aurora", agent: "deepseek", model: "deepseek-v4-pro", status: "attention", attention: "approval", phase: "approval", summary: "Run command? sqlx migrate run", cwd: "/srv/example/aurora", branch: "release/2.4", account: "acc-ds-b1" }),
  S({ key: "b1:2", name: "nightly-report", machine: "build-01", project: "aurora", agent: "claude", status: "scheduled", summary: "Next report at 07:00", scheduleNext: "07:00", scheduleCount: 1, cwd: "/srv/example/aurora", account: "acc-claude-b1" }),
  S({ git: { ahead: 0, behind: 0, changed: 2, state: "unknown" }, key: "ws:4", name: "accessibility", machine: "workstation", project: "aurora", agent: "kimi", model: "kimi-k2.5", status: "working", phase: "thinking", summary: "Reviewing focus order in the settings dialog", turnStartedMin: 12, branch: "feat/a11y", account: "acc-kimi-ws", draft: true }),
  S({ git: { ahead: 0, behind: 0, changed: 0, state: "unknown" }, key: "ws:9", name: "docs-sync", machine: "workstation", project: "aurora", agent: "claude", status: "waiting", summary: "Waiting for result from api-refactor on workstation", waitingFor: "api-refactor", parent: undefined, optionalQuestions: 1 }),
  S({ key: "mac:2", name: "release-notes", machine: "macbook", project: "aurora", agent: "claude", status: "ready", summary: "Draft for 2.4 is ready", cwd: "/Users/example/src/aurora", unread: true, attention: "reply", account: "acc-claude-mac" }),
  S({ git: { ahead: 0, behind: 4, changed: 1, state: "pull" }, key: "ws:5", name: "search-index", machine: "workstation", project: "aurora", agent: "opencode", model: "qwen3-coder", effort: undefined, status: "error", summary: "Provider rate limit; retry after 14:20", errorText: "429 from provider: retry after 14:20", account: "acc-oc-ws" }),
  // Infrastructure
  S({ git: { ahead: 1, behind: 0, changed: 3, conflicts: 1, state: "diff" }, key: "ws:6", name: "runner-images", machine: "workstation", project: "infra", agent: "codex", model: "gpt-6.1-sol", status: "working", phase: "tool", summary: "Edit: images/runner/Containerfile", tool: "Edit", turnStartedMin: 2, cwd: "/home/example/src/infra", branch: "chore/runner-26", account: "acc-codex-ws" }),
  S({ git: { ahead: 2, behind: 0, changed: 5, state: "unknown" }, key: "b2:1", name: "runner-cleanup", machine: "build-02", project: "infra", agent: "claude", status: "offline", summary: "Last known: idle", cwd: "/srv/example/runners", lastEvent: "18 min ago", account: "acc-claude-b2" }),
  S({ key: "b2:2", name: "disk-audit", machine: "build-02", project: "infra", agent: "deepseek", model: "deepseek-v4-pro", status: "offline", summary: "Last known: working on du report", cwd: "/srv/example/runners", lastEvent: "18 min ago", account: "acc-ds-b1" }),
  S({ key: "win:1", name: "installer-sign", machine: "win-lab", project: "infra", agent: "codex", model: "gpt-6.1-sol", status: "ready", summary: "MSI signed in test mode", cwd: "C:\\src\\example\\infra", account: "acc-codex-win", muted: true }),
  S({ key: "ws:7", name: "dns-rotation", machine: "workstation", project: "infra", agent: "claude", status: "scheduled", summary: "Every Monday 09:00", scheduleNext: "Mon 09:00", scheduleCount: 2, cwd: "/home/example/src/infra" }),
  // Docs & research
  S({ git: { ahead: 0, behind: 0, changed: 0, state: "synced" }, key: "mac:3", name: "handbook", machine: "macbook", project: "docs", agent: "claude", status: "ready", summary: "Updated the onboarding chapter", cwd: "/Users/example/notes/handbook", branch: undefined, account: "acc-claude-mac" }),
  S({ key: "mac:5", name: "style-guide", machine: "macbook", project: "docs", agent: "codex", model: "gpt-6.1-sol", status: "paused", summary: "Resume to continue", cwd: "/Users/example/notes/handbook", branch: undefined, account: "acc-codex-mac", reviewLater: true }),
  S({ key: "mac:4", name: "paper-notes", machine: "macbook", project: "docs", agent: "kimi", model: "kimi-k2.5", status: "ready", summary: "Summarized three papers", cwd: "/Users/example/notes/handbook", branch: undefined, account: "acc-kimi-mac", draft: true }),
  // General (default project)
  S({ key: "ws:8", name: "scratch", machine: "workstation", project: "general", agent: "claude", status: "ready", summary: "Idle", cwd: "/home/example/tmp", branch: undefined }),
];

export const archived: Session[] = [
  S({ key: "ws:a1", name: "onboarding-flow", machine: "workstation", project: "aurora", agent: "claude", status: "ended", summary: "Merged in #318", archivedAt: "Oct 24" }),
  S({ key: "mac:a2", name: "icon-refresh", machine: "macbook", project: "aurora", agent: "codex", model: "gpt-6.1-sol", status: "ended", summary: "New icon set exported", archivedAt: "Oct 22", account: "acc-codex-mac" }),
  S({ key: "b1:a3", name: "flaky-tests", machine: "build-01", project: "aurora", agent: "deepseek", model: "deepseek-v4-pro", status: "ended", summary: "Quarantined two tests", archivedAt: "Oct 19", account: "acc-ds-b1" }),
  S({ key: "ws:a4", name: "vpn-rollout", machine: "workstation", project: "infra", agent: "claude", status: "ended", summary: "Rolled out to all runners", archivedAt: "Oct 12" }),
];

export const accounts: Account[] = [
  { id: "acc-claude-ws", label: "Aurora engineering", agent: "claude", machine: "workstation", plan: "Max", fiveHour: 34, week: 18, resetIn: "2 h 10 min", sessions: 6, signedIn: true },
  { id: "acc-codex-ws", label: "Aurora engineering", agent: "codex", machine: "workstation", plan: "Pro", fiveHour: 52, week: 31, resetIn: "1 h 05 min", sessions: 2, signedIn: true },
  { id: "acc-codex-mac", label: "Release review", agent: "codex", machine: "macbook", plan: "Pro", fiveHour: 43, week: 25, resetIn: "3 h 40 min", sessions: 2, signedIn: true },
  { id: "acc-claude-mac", label: "Release review", agent: "claude", machine: "macbook", plan: "Max", fiveHour: 12, week: 9, resetIn: "4 h 30 min", sessions: 2, signedIn: true },
  { id: "acc-kimi-ws", label: "Research", agent: "kimi", machine: "workstation", plan: "Moderato", fiveHour: 61, week: 39, resetIn: "52 min", sessions: 1, signedIn: true },
  { id: "acc-ds-b1", label: "DeepSeek workspace", agent: "deepseek", machine: "build-01", plan: "API", fiveHour: 0, week: 0, resetIn: "-", sessions: 2, signedIn: true },
  { id: "acc-oc-ws", label: "Local models", agent: "opencode", machine: "workstation", plan: "Local", fiveHour: 0, week: 0, resetIn: "-", sessions: 1, signedIn: false },
];

export const tasks: Task[] = [
  { ref: "T-4k2a9q", title: "Paginate the v2 list endpoints", description: "Cursor pagination for /v2/orders and /v2/customers. Keep v1 unchanged and add contract tests.", stage: "active", project: "aurora", owner: "api-refactor", targetKind: "session", target: "api-refactor", dispatch: "submitted", comments: 2, updated: "4 min ago", history: [
    { at: "Oct 27 10:02", who: "You", kind: "created", text: "Created in To do" },
    { at: "Oct 27 10:05", who: "You", kind: "target", text: "Target: api-refactor on workstation" },
    { at: "Oct 27 10:06", who: "You", kind: "sent", text: "Sent to api-refactor" },
    { at: "Oct 27 10:06", who: "api-refactor", kind: "started", text: "Started" },
    { at: "Oct 27 11:40", who: "You", kind: "comment", text: "Keep page size at most 200." },
  ] },
  { ref: "T-8m1c3d", title: "Checkout form keyboard order", description: "Fields must follow street, city, postal code, country. Error text sits under each field.", stage: "review", project: "aurora", owner: "checkout-ui", targetKind: "session", target: "checkout-ui", dispatch: "submitted", comments: 1, updated: "12 min ago", history: [
    { at: "Oct 27 09:10", who: "You", kind: "created", text: "Created in To do" },
    { at: "Oct 27 09:12", who: "You", kind: "sent", text: "Sent to checkout-ui" },
    { at: "Oct 27 11:58", who: "checkout-ui", kind: "reported", text: "Reordered fields, added inline errors, 18 tests pass. Left: copy review for the Dutch locale." },
  ] },
  { ref: "T-2p7x0f", title: "Release notes for 2.4", description: "Summarize user-visible changes since 2.3 and link the migration guide.", stage: "review", project: "aurora", owner: "release-notes", targetKind: "session", target: "release-notes", dispatch: "submitted", attachments: 1, comments: 0, updated: "30 min ago", history: [
    { at: "Oct 27 08:30", who: "You", kind: "created", text: "Created in To do" },
    { at: "Oct 27 11:30", who: "release-notes", kind: "reported", text: "Draft ready for review." },
  ] },
  { ref: "T-9q4w2e", title: "Migrate session store to v3 schema", description: "Write the migration, dry-run it on a copy, then run it on build-01.", stage: "todo", project: "aurora", targetKind: "new-session", target: "New session on build-01", dispatch: "unsent", comments: 0, updated: "1 h ago", history: [
    { at: "Oct 27 07:55", who: "You", kind: "created", text: "Created in To do" },
    { at: "Oct 27 07:56", who: "You", kind: "target", text: "Planned: new session on build-01" },
  ] },
  { ref: "T-5t6y7u", title: "Flaky upload test on macOS", description: "Reproduce with 50 runs and fix the timing assumption.", stage: "todo", project: "aurora", targetKind: "any", dispatch: "unsent", comments: 0, updated: "2 h ago", history: [
    { at: "Oct 27 06:40", who: "You", kind: "created", text: "Created in To do" },
  ] },
  { ref: "T-3r8e1w", title: "Dark mode for the docs site", description: "Low priority. Use the existing palette.", stage: "backlog", project: "aurora", comments: 0, updated: "Oct 25", history: [
    { at: "Oct 25 16:00", who: "You", kind: "created", text: "Created in Backlog" },
  ] },
  { ref: "T-6h2j9k", title: "Evaluate cursor vs offset pagination", description: "Bench both on 10M rows.", stage: "backlog", project: "aurora", comments: 3, updated: "Oct 24", history: [
    { at: "Oct 24 12:00", who: "You", kind: "created", text: "Created in Backlog" },
  ] },
  { ref: "T-1a2s3d", title: "Upgrade runner base image", description: "Move runners to the 26.10 image.", stage: "active", project: "infra", owner: "runner-images", targetKind: "session", target: "runner-images", dispatch: "uncertain", comments: 0, updated: "6 min ago", history: [
    { at: "Oct 27 11:20", who: "You", kind: "created", text: "Created in To do" },
    { at: "Oct 27 11:21", who: "You", kind: "sent", text: "Delivery uncertain: review before sending again" },
  ] },
  { ref: "T-0z9x8c", title: "Rotate staging certificates", description: "Done via dns-rotation.", stage: "done", project: "infra", owner: "dns-rotation", comments: 1, updated: "Oct 26", history: [
    { at: "Oct 26 09:00", who: "dns-rotation", kind: "reported", text: "Rotated, verified with curl against staging.example.com." },
  ] },
  { ref: "T-7v6b5n", title: "Onboarding flow copy", description: "Shorter welcome text.", stage: "done", project: "aurora", owner: "onboarding-flow", comments: 0, updated: "Oct 24", history: [
    { at: "Oct 24 15:00", who: "onboarding-flow", kind: "reported", text: "Merged in #318." },
  ] },
];

export const activity: ActivityItem[] = [
  { id: "a1", kind: "user", at: "Oct 27 10:06", body: "Paginate the v2 list endpoints with cursors. Keep v1 responses exactly as they are and add contract tests for both.", sender: "You" },
  { id: "a1b", kind: "thinking", at: "Oct 27 10:06", body: "Plan: read the handlers, extract a shared cursor type, keep v1 untouched, then add contract tests." },
  { id: "a2", kind: "tools", at: "Oct 27 10:07", title: "4 tool calls, 2 events", body: "Read, Grep, Read, apply_patch", lines: ["Read  api/src/routes/orders.rs", "Grep  \"fn list_\" api/src", "Read  api/src/pagination.rs", "Edit  api/src/routes/orders.rs  +48 -12"] },
  { id: "a3", kind: "agent", at: "Oct 27 10:21", body: "The v2 handlers now share one cursor type.\n\n- `Cursor` encodes the last sort key and id, so ordering is stable while new rows arrive.\n- v1 still uses offset paging and its response shape is unchanged.\n- Page size is capped at **200**; larger requests get a 400 with a clear message." , code: "cargo test -p api pagination\n   Compiling api v2.4.0\n    Finished test [unoptimized] target(s) in 18.2s\n     Running 42 tests\ntest result: ok. 42 passed; 0 failed" },
  { id: "a3b", kind: "report", at: "Oct 27 10:40", title: "Subagent report", sender: "Regression tests", body: "42 checks passed on workstation and macbook. No v1 response shape changed." },
  { id: "a3c", kind: "voice", at: "Oct 27 11:02", sender: "You (voice)", body: "Keep the old offset parameter working for one more release, but log a deprecation warning." },
  { id: "a4", kind: "receipt", at: "Oct 27 11:40", body: "Task comment delivered: Keep page size at most 200.", sender: "Tasks" },
  { id: "a5", kind: "user", at: "Oct 27 11:41", body: "Also check the contract tests against the staging fixture at https://staging.example.com/v2.", sender: "You", attachments: [{ name: "fixture-orders.json", size: "18 KB", kind: "file" }, { name: "screenshot.png", size: "240 KB", kind: "image" }] },
  { id: "a6", kind: "tool", at: "Oct 27 11:43", title: "Tool started / Bash", body: "cargo test --workspace" },
];

export const terminalLines = [
  "\x1b[38;5;245m~/src/aurora\x1b[0m \x1b[38;5;114m(feat/api-v2)\x1b[0m $ cargo test --workspace",
  "\x1b[1;32m   Compiling\x1b[0m pager v0.3.1 (/home/example/src/aurora/crates/pager)",
  "\x1b[1;32m   Compiling\x1b[0m api v2.4.0 (/home/example/src/aurora/api)",
  "\x1b[1;32m    Finished\x1b[0m test [unoptimized + debuginfo] target(s) in 41.07s",
  "\x1b[1;32m     Running\x1b[0m unittests src/lib.rs (target/debug/deps/api-5c1e)",
  "",
  "running 42 tests",
  "test pagination::cursor_round_trip ... \x1b[32mok\x1b[0m",
  "test pagination::stable_order_with_inserts ... \x1b[32mok\x1b[0m",
  "test routes::orders::v1_shape_unchanged ... \x1b[32mok\x1b[0m",
  "test routes::orders::page_size_capped ... \x1b[32mok\x1b[0m",
  "test routes::customers::v2_cursor ... \x1b[33mrunning\x1b[0m",
];

export const processes = [
  { pid: 48211, command: "cargo test --workspace", state: "running", cpu: 312, mem: "1.4 GiB", started: "11:43", kind: "Tool" },
  { pid: 48305, command: "rustc --crate-name api …", state: "running", cpu: 98, mem: "620 MiB", started: "11:43", kind: "Child" },
  { pid: 47110, command: "pnpm dev --port 5173", state: "running", cpu: 2, mem: "210 MiB", started: "10:12", kind: "Background shell" },
  { pid: 46002, command: "claude (native agent)", state: "running", cpu: 4, mem: "380 MiB", started: "10:02", kind: "Agent" },
];

/** A large synthetic catalog for the virtualization stress mode. */
export function stressSessions(count: number): Session[] {
  const agents = ["claude", "codex", "kimi", "deepseek", "opencode"] as const;
  const statuses = ["working", "ready", "ready", "ready", "background", "scheduled", "attention", "ready"] as const;
  const hosts = ["workstation", "macbook", "build-01", "win-lab"];
  const out: Session[] = [];
  for (let i = 0; i < count; i++) {
    const status = statuses[i % statuses.length];
    out.push(S({
      key: `stress:${i}`,
      name: `job-${String(i).padStart(5, "0")}`,
      machine: hosts[i % hosts.length],
      project: i % 3 === 0 ? "infra" : "aurora",
      agent: agents[i % agents.length],
      status,
      attention: status === "attention" ? "question" : undefined,
      summary: status === "working" ? `Bash: make check-${i % 17}` : `Finished batch ${i}`,
      turnStartedMin: status === "working" ? i % 40 : undefined,
      unread: i % 11 === 0,
      parent: i % 9 === 1 ? `stress:${i - 1}` : undefined,
    }));
  }
  return out;
}
