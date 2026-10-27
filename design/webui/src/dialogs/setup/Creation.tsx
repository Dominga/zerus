import { For, Show, createMemo, createSignal, onCleanup } from "solid-js";
import { AGENT_LABEL, Icon } from "../../lib/icons";
import { CREATION_CAPTION, type CreationRecord, type CreationState } from "../../mock/setup-extra";
import { setPage, toast } from "../../state";
import { creations, setCreations, projectById, setLaunchPreset } from "../../screens/setup/store";
import { SDialog, closeSetup, openSetup, Progress } from "../../screens/setup/ui";

// Session creation controller (SessionCreationController.*): a durable
// background pipeline that outlives the New session dialog. Stop keeps files
// and branches; an unknown clone or worktree result is never replayed and only
// offers Check result.

export interface CreationPlan {
  tag: string;
  project: string;
  machine: string;
  agent: CreationRecord["agent"];
  folder: string;
  worktree?: { path: string; branch: string };
  model?: string;
  repository?: string;
  predecessor?: string;
}

const STEP_MS = 1500;
const RUNNING: CreationState[] = ["saving", "queued", "checking", "checking_repository", "cloning", "worktree", "verifying", "launching", "confirming", "stopping"];
const SETTLED: CreationState[] = ["stopped", "unknown", "storage_error", "failed", "bound"];
const stopAfterPreparation = (s: CreationState) => s === "cloning" || s === "worktree";

const timers = new Map<string, ReturnType<typeof setTimeout>>();
const stopRequested = new Set<string>();
const clock = () => new Date().toTimeString().slice(0, 8);

function stages(r: CreationRecord): CreationState[] {
  const base: CreationState[] = r.repository ? ["saving", "queued", "checking_repository", "cloning", "verifying"] : ["saving", "queued", "checking"];
  if (r.worktree) base.push("worktree", "verifying");
  return [...new Set(base), "launching", "confirming", "bound"];
}

function setState(id: string, state: CreationState, extra: Partial<CreationRecord> = {}) {
  setCreations("items", (c) => c.id === id, (c) => ({
    ...c, ...extra, state, lastStage: c.state, elapsed: 0, startedAt: Date.now(), endedAt: SETTLED.includes(state) ? Date.now() : undefined,
    history: [...c.history, { at: clock(), text: CREATION_CAPTION[state] }].slice(-32),
  }));
}

function advance(id: string) {
  const r = creations.items.find((c) => c.id === id);
  if (!r || SETTLED.includes(r.state)) return;
  if (stopRequested.has(id) && (r.state === "stopping" || !stopAfterPreparation(r.state))) {
    stopRequested.delete(id);
    setState(id, "stopped", { error: "Preparation ended. The checkout and branch are kept." });
    return;
  }
  const list = stages(r);
  const next = list[list.indexOf(r.state) + 1] ?? "bound";
  setState(id, stopRequested.has(id) ? "stopping" : next);
  if (next === "bound") toast("Session created", `${r.tag} on ${r.machine}`);
  timers.set(id, setTimeout(() => advance(id), STEP_MS));
}

export function startCreation(plan: CreationPlan): CreationRecord {
  const record: CreationRecord = {
    id: `c-${Date.now().toString(36)}`, tag: plan.tag, project: plan.project, machine: plan.machine, agent: plan.agent,
    folder: plan.folder, worktree: plan.worktree, model: plan.model, repository: plan.repository, state: "saving", startedAt: Date.now(), createdAt: Date.now(),
    created: new Date().toLocaleString("en-GB", { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" }), elapsed: 0,
    history: [{ at: clock(), text: CREATION_CAPTION.saving }],
  };
  // A request is saved durably before any step runs; a predecessor stays visible.
  setCreations("items", (items) => [record, ...items].slice(0, 128));
  setCreations("selected", record.id);
  timers.set(record.id, setTimeout(() => advance(record.id), STEP_MS));
  return record;
}

function details(r: CreationRecord, taking: boolean): string {
  const lines = [
    r.tag,
    `Project: ${projectById(r.project)?.name ?? r.project}`,
    `Computer: ${r.machine}`,
    `Agent: ${r.agent === "shell" ? "Shell" : AGENT_LABEL[r.agent]}`,
    `Folder: ${r.folder}`,
    ...(r.repository ? [`Repository: ${r.repository}`] : []),
    ...(r.model ? [`Model: ${r.model}`] : []),
    `Stage: ${CREATION_CAPTION[r.state]}${taking ? " — Taking longer than usual…" : ""}`,
    ...(r.error ? [`Error: ${r.error}`] : []),
    ...(r.lastStage && SETTLED.includes(r.state) ? [`Last stage: ${CREATION_CAPTION[r.lastStage]}`] : []),
    ...(r.worktree ? [`Worktree: ${r.worktree.path}`, `Branch: ${r.worktree.branch}`] : []),
    `Created: ${r.created}`,
    "",
    "History",
    ...r.history.map((h) => `  ${h.at}  ${h.text}`),
  ];
  return lines.join("\n");
}

function CreationPanel() {
  const [now, setNow] = createSignal(Date.now());
  const tick = setInterval(() => setNow(Date.now()), 1000);
  onCleanup(() => clearInterval(tick));
  const r = createMemo(() => creations.items.find((c) => c.id === creations.selected));
  const since = (c: CreationRecord) => c.startedAt ?? now();
  const mmss = (c: CreationRecord) => {
    const s = Math.max(0, Math.floor(((SETTLED.includes(c.state) ? c.endedAt ?? now() : now()) - (c.createdAt ?? now())) / 1000));
    return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
  };
  const running = () => !!r() && RUNNING.includes(r()!.state);
  const taking = () => running() && now() - since(r()!) > 30000;
  const stop = () => {
    const c = r(); if (!c) return;
    stopRequested.add(c.id);
    if (!stopAfterPreparation(c.state)) { clearTimeout(timers.get(c.id)); setState(c.id, "stopping"); timers.set(c.id, setTimeout(() => advance(c.id), 800)); }
    else toast("Stop after preparation", "Later steps are fenced; the current step finishes first");
  };
  const check = () => {
    const c = r(); if (!c) return;
    toast("Check result", "Reading the original result. The operation will not be repeated.");
    setTimeout(() => setState(c.id, "recover", { error: undefined, history: [...c.history, { at: clock(), text: `Original result confirmed: ${c.worktree?.path ?? c.folder}` }] }), 900);
  };
  const resume = () => { const c = r(); if (!c) return; setState(c.id, "verifying"); timers.set(c.id, setTimeout(() => advance(c.id), STEP_MS)); };
  const edit = () => { const c = r(); if (!c) return; setLaunchPreset({ restore: c }); closeSetup("setup-creation"); openSetup("new-session"); };
  const hide = () => {
    const c = r(); if (!c) return;
    const rest = creations.items.filter((x) => x.id !== c.id);
    setCreations({ items: rest, selected: rest[0]?.id ?? "" });
  };
  return (
    <>
      <div class="dlg-body cr-body">
        <ul class="cr-list" role="listbox" aria-label="Session creations">
          <For each={creations.items} fallback={<li class="hint cr-empty">No pending session creations</li>}>
            {(c) => (
              <li role="option" aria-selected={c.id === creations.selected} classList={{ on: c.id === creations.selected }} onClick={() => setCreations("selected", c.id)}
                title={c.state === "bound" ? "This creation has already been resolved." : undefined}>
                <Icon name={c.state === "bound" ? "check" : c.state === "failed" || c.state === "storage_error" ? "circle-alert" : c.state === "unknown" ? "circle-question-mark" : c.state === "stopped" ? "square" : "loader-circle"}
                  size={13} class={c.state === "bound" ? "accent" : c.state === "failed" || c.state === "storage_error" ? "danger" : c.state === "unknown" ? "warn" : RUNNING.includes(c.state) ? "su-spin dim" : "dim"} />
                <span class="ell"><b>{c.tag}</b> · {CREATION_CAPTION[c.state]}</span>
              </li>
            )}
          </For>
        </ul>
        <Show when={r()} fallback={<div class="empty"><Icon name="clock" size={22} /><p>No pending session creations</p></div>}>
          {(c) => (
            <section class="cr-panel" aria-live="polite">
              <h3 class="cr-title tabular">{c().tag} · {CREATION_CAPTION[c().state]} · {mmss(c())}</h3>
              <Show when={running()}><Progress label={CREATION_CAPTION[c().state]} /></Show>
              <Show when={c().error}>
                <div class="notice" classList={{ warn: c().state !== "failed" && c().state !== "storage_error", danger: c().state === "failed" || c().state === "storage_error" }}>
                  <Icon name={c().state === "failed" ? "circle-alert" : "triangle-alert"} size={14} /><span>{c().error}</span>
                </div>
              </Show>
              <pre class="cr-log mono" tabindex="0">{details(c(), taking())}</pre>
              <div class="row cr-actions">
                <Show when={c().state === "recover"}><button class="btn primary small" onClick={resume}>Continue setup</button></Show>
                <Show when={c().state === "unknown"}><button class="btn primary small" onClick={check}>Check result</button></Show>
                <Show when={running() && !["launching", "confirming", "stopping"].includes(c().state)}>
                  <button class="btn small" onClick={stop}>{stopAfterPreparation(c().state) ? "Stop after preparation" : "Stop creating"}</button>
                </Show>
                <Show when={["failed", "stopped", "storage_error", "unknown"].includes(c().state)}><button class="btn small" onClick={edit}>Edit choices…</button></Show>
                <span class="grow" />
                <Show when={!running()}><button class="btn small ghost" onClick={hide}>Hide status</button></Show>
                <button class="btn small ghost" onClick={() => { closeSetup("setup-creation"); setPage("sessions"); }}>Return to filters</button>
              </div>
            </section>
          )}
        </Show>
      </div>
    </>
  );
}

export function CreationDialog() {
  return (
    <SDialog id="setup-creation" title="Session creation" width="920px" icon="clock" bare>
      <CreationPanel />
    </SDialog>
  );
}
