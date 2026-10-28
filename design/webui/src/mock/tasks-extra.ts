// Synthetic project tasks for the Tasks mockup. The shared `tasks` array is
// the base; this file enriches it into the view model `hgs task get` would
// give the client (targets, executors, discussion, history, attachments,
// recovery markers) and adds synthetic cases: blockers, subagents, a
// cancelled review, a read-only future-schema card, older Done items, an
// uncertain dispatch and a long history. All names and paths are invented.
import { tasks as sharedTasks, sessions, archived, machines } from "./data";
import type { Task, TaskStage } from "./types";

export type { TaskStage };
export type TargetKind = "session" | "any" | "new_session" | "subagent";

export interface TaskFile { ref: string; name: string; size: string; kind: "image" | "file" }

/** One journal record, as `taskHistoryAction` names its kind. */
export interface TaskLog {
  at: string;
  actor: string; // "You" or a session label
  agent?: boolean;
  kind: "created" | "title" | "edited" | "stage" | "target" | "sent" | "report" | "cancelled" | "comment" | "message_delivery" | "resumed" | "started";
  text?: string;
  files?: TaskFile[];
}

/** Delivery states of a discussion record (TaskInspector.cpp:714-745). */
export type Delivery = "note" | "queued" | "submitted-queue" | "submitted" | "submitted-coordinator" | "failed" | "unconfirmed";

export interface TaskNote {
  id: string;
  author: string;
  agent?: boolean;
  kind: "note" | "message";
  text: string;
  at: string;
  delivery: Delivery;
  error?: string;
  files?: TaskFile[];
}

export interface TaskTarget {
  kind: TargetKind;
  host?: string; // "" for this computer, else its SSH alias; the parent for new_session / subagent
  session?: string;
  known?: boolean; // false: a computer this one has not verified, or a lost identity
  idLost?: boolean;
}

export interface TaskVM {
  id: string;
  project: string;
  title: string;
  text: string;
  stage: TaskStage;
  writable: boolean;
  target: TaskTarget;
  executor?: { host: string; name: string; known?: boolean };
  startConfirmed?: boolean;
  worker?: string;
  author?: { host: string; name: string };
  created: string;
  stageAgeMin?: number;
  doneDaysAgo?: number;
  acceptedOn?: string;
  summary?: string;
  reportedAt?: string;
  cancelledAt?: string;
  cancelReason?: string;
  files: TaskFile[];
  discussion: TaskNote[];
  log: TaskLog[];
  logTotal?: number; // full journal length; the client retains at most 80
  olderLog?: TaskLog[]; // the page before the retained window, read on "Earlier events…"
  recovery?: boolean; // a private outbox record waits for "Recover saved comment"
  launch?: { phase: "waiting" | "review"; session: string; launch: string; error: string };
}

export interface TaskSettings { task_template: string; cancel_template: string; agents_other: boolean; agent_limit: number }

const everyone = [...sessions, ...archived];
const thisComputer = machines.find((m) => m.thisComputer)?.alias;
export const hostOf = (machine: string) => (machine === thisComputer ? "" : machine);
const sessionHost = (name?: string) => {
  const s = everyone.find((x) => x.name === name);
  return s ? hostOf(s.machine) : "";
};

const HISTORY_KIND: Record<Task["history"][number]["kind"], TaskLog["kind"]> = {
  created: "created", edited: "edited", sent: "sent", started: "started", reported: "report",
  comment: "comment", moved: "stage", target: "target", reopened: "resumed",
};

function fromShared(t: Task): TaskVM {
  const host = sessionHost(t.owner ?? t.target);
  const targetKind: TargetKind = t.targetKind === "new-session" ? "new_session" : t.targetKind ?? "any";
  const sent = t.stage !== "todo" && t.stage !== "backlog" && t.owner;
  return {
    id: t.ref,
    project: t.project,
    title: t.title,
    text: t.description,
    stage: t.stage,
    writable: true,
    target: targetKind === "any" ? { kind: "any" } : { kind: targetKind, host, session: t.owner ?? t.target },
    executor: sent ? { host, name: t.owner! } : undefined,
    startConfirmed: t.dispatch !== "uncertain",
    created: t.history[0]?.at ?? t.updated,
    files: [],
    discussion: [],
    log: t.history.map((h) => ({
      at: h.at,
      actor: h.who === "You" ? "You" : h.who + (sessionHost(h.who) ? ` @${sessionHost(h.who)}` : ""),
      agent: h.who !== "You",
      kind: HISTORY_KIND[h.kind],
      text: h.kind === "moved" ? h.text.replace(/^Moved to /, "")
        : h.kind === "target" ? h.text.replace(/^(Target|Planned): /, "")
        : h.kind === "sent" ? (h.text.startsWith("Sent to ") ? h.text.slice(8) : t.owner ?? h.text)
        : h.text,
    })),
  };
}

const F = {
  fixture: { ref: "[File #1]", name: "fixture-orders.json", size: "18 KB", kind: "file" } as TaskFile,
  shot: { ref: "[Image #1]", name: "screenshot.png", size: "240 KB", kind: "image" } as TaskFile,
  notes: { ref: "[File #1]", name: "release-notes-2.4.md", size: "6 KB", kind: "file" } as TaskFile,
  trace: { ref: "[File #1]", name: "upload-trace.txt", size: "41 KB", kind: "file" } as TaskFile,
};

/** A long journal: `count` events, the newest last. */
function longLog(count: number, actor: string): TaskLog[] {
  const out: TaskLog[] = [{ at: "Sep 30 09:12", actor: "You", kind: "created" }];
  const kinds: TaskLog["kind"][] = ["comment", "edited", "comment", "stage", "comment", "target"];
  for (let i = 1; i < count - 3; i++) {
    const kind = kinds[i % kinds.length];
    const day = 1 + Math.floor(i / 6);
    out.push({
      at: `Oct ${String(Math.min(day, 25)).padStart(2, "0")} ${String(8 + (i % 10)).padStart(2, "0")}:${String((i * 7) % 60).padStart(2, "0")}`,
      actor: i % 4 === 0 ? actor : "You",
      agent: i % 4 === 0,
      kind,
      text: kind === "comment" ? `Review note ${i}: checked export callers in batch ${i}.` : kind === "stage" ? (i % 2 ? "backlog" : "todo") : kind === "target" ? actor : undefined,
    });
  }
  out.push({ at: "Oct 25 10:04", actor: "You", kind: "sent", text: actor });
  out.push({ at: "Oct 25 10:05", actor, agent: true, kind: "started" });
  out.push({ at: "Oct 25 15:40", actor, agent: true, kind: "report", text: "Removed /v1/export and its two internal callers; migration note added." });
  return out;
}

const LONG = longLog(112, "api-refactor");

const ENRICH: Record<string, Partial<TaskVM>> = {
  "T-4k2a9q": {
    text: "Cursor pagination for /v2/orders and /v2/customers. Keep v1 unchanged and add contract tests.\n\nUse [File #1] as the response fixture. Page size stays capped; larger requests get a clear 400.",
    files: [F.fixture],
    stageAgeMin: 97,
    discussion: [
      { id: "n1", author: "You", kind: "message", text: "Keep page size at most 200.", at: "Oct 27 11:40", delivery: "submitted" },
      { id: "n2", author: "You", kind: "message", text: "Also check the contract tests against the staging fixture at https://staging.example.com/v2.", at: "Oct 27 11:41", delivery: "submitted", files: [F.shot] },
      { id: "n3", author: "api-refactor", agent: true, kind: "note", text: "Shared one Cursor type for orders and customers; v1 handlers are untouched.", at: "Oct 27 11:44", delivery: "note" },
      { id: "n4", author: "You", kind: "note", text: "Follow up on rate limits after this lands.", at: "Oct 27 11:50", delivery: "note" },
    ],
  },
  "T-8m1c3d": {
    summary: "Reordered fields, added inline errors, 18 tests pass. Left: copy review for the Dutch locale.",
    reportedAt: "Oct 27 11:58",
    discussion: [{ id: "n1", author: "You", kind: "note", text: "Dutch copy goes to the localization team.", at: "Oct 27 12:02", delivery: "note" }],
  },
  "T-2p7x0f": { summary: "Draft ready for review. Breaking changes are listed first; see [File #1].", reportedAt: "Oct 27 11:30", files: [F.notes] },
  "T-9q4w2e": {
    target: { kind: "new_session", host: "build-01", session: "migrate-db" },
    launch: { phase: "review", session: "aurora/migrate-v3", launch: "launch-7f3a2c91", error: "Task delivery reply did not confirm this exact executor and receipt" },
    discussion: [{ id: "n1", author: "You", kind: "message", text: "Dry-run against the 24 Oct snapshot first.", at: "Oct 27 08:02", delivery: "queued" }],
  },
  "T-5t6y7u": { files: [F.trace], text: "Reproduce with 50 runs and fix the timing assumption. The failing trace is [File #1]." },
  "T-6h2j9k": {
    target: { kind: "session", host: "", session: "cursor-bench" },
    discussion: [
      { id: "n1", author: "You", kind: "note", text: "Use the 10M-row fixture on build-01.", at: "Oct 24 12:10", delivery: "note" },
      { id: "n2", author: "cursor-bench", agent: true, kind: "note", text: "Offset paging degrades past page 400; cursor stays flat.", at: "Oct 24 14:31", delivery: "note" },
      { id: "n3", author: "You", kind: "note", text: "Keep the raw numbers in the PR description.", at: "Oct 24 14:40", delivery: "note" },
    ],
  },
  "T-1a2s3d": {
    stageAgeMin: 6,
    recovery: true,
    discussion: [
      { id: "n1", author: "You", kind: "message", text: "Use the 26.10.2 digest, not latest.", at: "Oct 27 11:24", delivery: "unconfirmed" },
      { id: "n2", author: "You", kind: "message", text: "Rebuild the arm64 variant too.", at: "Oct 27 11:31", delivery: "failed", error: "execution changed" },
    ],
  },
  "T-0z9x8c": { summary: "Rotated, verified with curl against staging.example.com.", doneDaysAgo: 1 },
  "T-7v6b5n": { summary: "Merged in #318.", doneDaysAgo: 3 },
};

const done = (id: string, title: string, days: number, accepted: string, executor: string): TaskVM => ({
  id, project: "aurora", title, text: title + ".", stage: "done", writable: true,
  target: { kind: "session", host: "", session: executor }, executor: { host: "", name: executor }, startConfirmed: true,
  created: "Sep 22 10:00", doneDaysAgo: days, acceptedOn: accepted, summary: "Done; tests pass.", files: [], discussion: [],
  log: [{ at: "Sep 22 10:00", actor: "You", kind: "created" }, { at: "Sep 23 09:00", actor: executor, agent: true, kind: "report", text: "Done; tests pass." }, { at: accepted.replace(/(\d+) (\w+)/, "$2 $1") + " 09:30", actor: "You", kind: "stage", text: "done" }],
});

const EXTRA: TaskVM[] = [
  {
    id: "T-q7w8e9", project: "aurora", title: "Contract tests for v1 orders", stage: "todo", writable: true,
    text: "Add contract tests that pin the v1 orders response. Compare against [File #1] and the screenshot [Image #1].",
    target: { kind: "session", host: "", session: "api-refactor" }, author: { host: "", name: "api-refactor-fork" },
    created: "Oct 27 10:48", files: [F.fixture, F.shot], discussion: [],
    log: [{ at: "Oct 27 10:48", actor: "api-refactor-fork", agent: true, kind: "created" }],
  },
  {
    id: "T-c6v5b4", project: "aurora", title: "Persist checkout draft between reloads", stage: "todo", writable: true,
    text: "Store the unsent checkout form in session storage and restore it after a reload.",
    target: { kind: "session", host: "macbook", session: "checkout-ui" }, created: "Oct 27 09:40", files: [], discussion: [],
    log: [{ at: "Oct 27 09:40", actor: "You", kind: "created" }, { at: "Oct 27 09:41", actor: "You", kind: "target", text: "checkout-ui @macbook" }],
  },
  {
    id: "T-o1i2u3", project: "aurora", title: "Warm the response cache after deploy", stage: "todo", writable: true,
    text: "Run the warmup script after each deploy and log hit rates.",
    target: { kind: "session", host: "build-01", session: "cache-spike" }, created: "Oct 26 17:20", files: [], discussion: [],
    log: [{ at: "Oct 26 17:20", actor: "You", kind: "created" }, { at: "Oct 26 17:21", actor: "You", kind: "target", text: "cache-spike @build-01" }],
  },
  {
    id: "T-z1x2c3", project: "aurora", title: "Pager crate: stable order with inserts", stage: "active", writable: true,
    text: "Prove that cursor order stays stable while rows are inserted between pages.",
    target: { kind: "subagent", host: "", session: "api-refactor" }, executor: { host: "", name: "api-refactor" }, worker: "Regression tests",
    startConfirmed: true, stageAgeMin: 22, created: "Oct 27 11:10", files: [], discussion: [
      { id: "n1", author: "You", kind: "message", text: "Include a test with 1 000 concurrent inserts.", at: "Oct 27 11:25", delivery: "submitted-coordinator" },
    ],
    log: [{ at: "Oct 27 11:10", actor: "You", kind: "created" }, { at: "Oct 27 11:12", actor: "You", kind: "target", text: "subagent of api-refactor" }, { at: "Oct 27 11:20", actor: "You", kind: "sent", text: "api-refactor" }, { at: "Oct 27 11:21", actor: "api-refactor", agent: true, kind: "started" }],
  },
  {
    id: "T-m4n5b6", project: "aurora", title: "Focus order audit for the settings dialog", stage: "active", writable: true,
    text: "List every focus stop in the settings dialog and fix the ones that skip controls.",
    target: { kind: "subagent", host: "", session: "accessibility" }, executor: { host: "", name: "accessibility" },
    startConfirmed: false, stageAgeMin: 8, created: "Oct 27 11:30", files: [], discussion: [],
    log: [{ at: "Oct 27 11:30", actor: "You", kind: "created" }, { at: "Oct 27 11:35", actor: "You", kind: "sent", text: "accessibility" }],
  },
  {
    id: "T-r5e4w3", project: "aurora", title: "Drop the legacy offset parameter", stage: "review", writable: true,
    text: "Remove ?offset= from v2 and document the cursor replacement.",
    target: { kind: "session", host: "", session: "api-refactor-fork" }, executor: { host: "", name: "api-refactor-fork" }, startConfirmed: true,
    cancelledAt: "11:52", cancelReason: "Wrong approach — keep offset paging for v1 clients.", created: "Oct 27 10:15", files: [], discussion: [
      { id: "n1", author: "You", kind: "note", text: "Wrong approach — keep offset paging for v1 clients.", at: "Oct 27 11:52", delivery: "note" },
    ],
    log: [{ at: "Oct 27 10:15", actor: "You", kind: "created" }, { at: "Oct 27 10:20", actor: "You", kind: "sent", text: "api-refactor-fork" }, { at: "Oct 27 11:52", actor: "You", kind: "cancelled" }, { at: "Oct 27 11:52", actor: "You", kind: "comment", text: "Wrong approach — keep offset paging for v1 clients." }],
  },
  {
    id: "T-l2k3j4", project: "aurora", title: "Shared filter presets", stage: "backlog", writable: false,
    text: "Created by a newer Zerus; this version shows it read-only.",
    target: { kind: "any" }, created: "Oct 26 12:00", files: [], discussion: [],
    log: [{ at: "Oct 26 12:00", actor: "You", kind: "created" }],
  },
  {
    id: "T-h8g7f6", project: "aurora", title: "Remove deprecated /v1/export", stage: "done", writable: true,
    text: "Remove the endpoint, its callers and the feature flag.",
    target: { kind: "session", host: "", session: "api-refactor" }, executor: { host: "", name: "api-refactor" }, startConfirmed: true,
    doneDaysAgo: 2, summary: "Removed /v1/export and its two internal callers; migration note added.", created: "Sep 30 09:12", files: [], discussion: [],
    log: LONG.slice(-80), olderLog: LONG.slice(0, -80), logTotal: 112,
  },
  done("T-a9s8d7", "Rate-limit headers on v1", 12, "15 Oct", "api-refactor"),
  done("T-f6g5h4", "Cache warmup on deploy", 20, "7 Oct", "search-index"),
  done("T-j3k2l1", "Error codes for checkout API", 31, "26 Sep", "checkout-ui"),
  // Infrastructure
  {
    id: "T-u7y6t5", project: "infra", title: "Verify MSI signing on the lab machine", stage: "todo", writable: true,
    text: "Sign the installer with the test certificate and check SmartScreen output.",
    target: { kind: "session", host: "win-lab", session: "installer-sign", known: false }, created: "Oct 27 10:00", files: [], discussion: [],
    log: [{ at: "Oct 27 10:00", actor: "You", kind: "created" }],
  },
  {
    id: "T-d4f5g6", project: "infra", title: "Prune runner caches older than 14 days", stage: "todo", writable: true,
    text: "Remove cache folders older than 14 days on every runner; keep the last two images.",
    target: { kind: "session", host: "build-02", session: "runner-cleanup" }, created: "Oct 27 09:00", files: [], discussion: [],
    log: [{ at: "Oct 27 09:00", actor: "You", kind: "created" }],
  },
];

/** Every task in board rank order: each stage's tasks by rank. */
export function seedTasks(): TaskVM[] {
  const base = sharedTasks.map((t) => ({ ...fromShared(t), ...ENRICH[t.ref] }));
  const all = [...base, ...EXTRA];
  // Interleave the extras into stage order the way `hgs task list` ranks them.
  const order = ["T-4k2a9q", "T-z1x2c3", "T-m4n5b6", "T-8m1c3d", "T-r5e4w3", "T-2p7x0f", "T-q7w8e9", "T-9q4w2e", "T-5t6y7u", "T-c6v5b4", "T-o1i2u3"];
  return all.sort((a, b) => {
    const ia = order.indexOf(a.id), ib = order.indexOf(b.id);
    return (ia < 0 ? 99 : ia) - (ib < 0 ? 99 : ib);
  });
}

/** Extra Backlog cards for the 20k stress toggle: shows the 100-card window. */
export function stressTasks(count: number): TaskVM[] {
  return Array.from({ length: count }, (_, i) => ({
    id: `T-s${String(i).padStart(5, "0")}`, project: "aurora", title: `Synthetic backlog item ${i + 1}`, stage: "backlog" as const, writable: true,
    text: `Synthetic backlog item ${i + 1}.`, target: { kind: "any" as const }, created: "Oct 20 09:00", files: [], discussion: [],
    log: [{ at: "Oct 20 09:00", actor: "You", kind: "created" as const }],
  }));
}

/** Damaged task files per project (listing `damaged`). */
export const damagedFiles: Record<string, number> = { infra: 1 };

export const taskSettings: Record<string, TaskSettings> = {
  aurora: {
    task_template: "Task {ref} from {author}:\n\n{prompt}\n\nWhen it is done, report with:\n  {tools} report {ref} < summary.txt\n\nHistory so far:\n{history}",
    cancel_template: "Task {ref} was cancelled by {author}. Stop working on it and report what you changed:\n  {tools} report {ref} < summary.txt",
    agents_other: true,
    agent_limit: 0,
  },
};
export const defaultSettings: TaskSettings = { ...taskSettings.aurora };
