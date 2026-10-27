import { For, Show, Switch, Match } from "solid-js";
import { Icon } from "../../lib/icons";
import { selected, inspectorTab, setInspectorTab, setInspectorOpen, setSelectedKey, toast, setDialog, type InspectorTab } from "../../state";
import { accounts, projects } from "../../mock/data";
import { TasksPanel, TaskTabBadge } from "../tasks/TasksPanel";

const TABS: { id: InspectorTab; label: string }[] = [
  { id: "plan", label: "Plan & agents" },
  { id: "details", label: "Details" },
  { id: "worktrees", label: "Worktrees" },
  { id: "tasks", label: "Tasks" },
];

function PlanAgents() {
  const s = () => selected();
  const kids = () => s()?.subagents ?? [];
  return (
    <div class="insp-body">
      <section class="insp-sec">
        <div class="insp-kv"><b>Main agent</b><span class="muted">{s()?.status === "working" ? "Using a tool" : "Ready"}</span></div>
        <div class="muted mono insp-small">{s()?.summary}</div>
      </section>
      <Show when={s()?.goal}>
        <section class="insp-sec goal">
          <div class="eyebrow">Goal</div>
          <div class="goal-line"><span class="sbadge k-scheduled"><i class="sb-ico" /><span class="sb-label">Pursuing goal</span></span><span class="dim">1 h 12 min · 38 turns</span></div>
          <p>{s()?.goal}</p>
          <div class="hint">Reported by the agent. Completion: all v1 contract tests pass.</div>
        </section>
      </Show>
      <section class="insp-sec">
        <div class="insp-h"><b>Tasks</b><span class="dim">2 of 3 completed (last recorded)</span></div>
        <ul class="plan-list">
          <li class="done"><Icon name="check" size={13} /><span>Inspect routes and existing clients<small>Done</small></span></li>
          <li class="done"><Icon name="check" size={13} /><span>Refactor handlers and add tests<small>Done</small></span></li>
          <li class="doing"><Icon name="circle-dot" size={13} /><span>Review macOS and server behavior<small>In progress</small></span></li>
        </ul>
      </section>
      <section class="insp-sec">
        <div class="insp-h"><b>Subagents</b><span class="dim">{kids().filter((k) => k.state === "working").length} active / {kids().length} total</span></div>
        <Show when={kids().length} fallback={<div class="hint">No subagent activity has been reported yet.</div>}>
          <ul class="agent-list">
            <For each={kids()}>{(k) => (
              <li>
                <span class={`state-dot ${k.state}`} />
                <button class="link" onClick={() => setSelectedKey(`${s()!.key}/${k.id}`)}>{k.name} →</button>
                <span class="dim ell">{k.summary}</span>
                <span class={`agent-state ${k.state}`}>{k.state === "working" ? "Working" : k.state === "ready" ? "Ready" : "Needs input"}</span>
              </li>
            )}</For>
          </ul>
        </Show>
      </section>
    </div>
  );
}

function Details() {
  const s = () => selected();
  const acc = () => accounts.find((a) => a.id === s()?.account);
  const row = (k: string, v: string | undefined, mono = false) => (
    <div class="kv"><span>{k}</span><span classList={{ mono }}>{v ?? "Not reported"}</span></div>
  );
  return (
    <div class="insp-body">
      <section class="insp-sec idblock">
        <div class="eyebrow">Session ID</div>
        <div class="row"><span class="mono ell">5c1e9b2a-7d40-4e6b-9a1f-2b8c0d3e4f51</span><button class="icon-btn small" title="Copy" onClick={() => toast("Copied", "Session ID copied")}><Icon name="copy" size={12} /></button></div>
      </section>
      <section class="insp-sec">
        <div class="eyebrow">Session</div>
        {row("Name", `${s()?.agent}/${s()?.project}/${s()?.name}`, true)}
        {row("Machine", s()?.machine)}
        {row("Directory", s()?.cwd, true)}
        {row("Branch", s()?.branch)}
        {row("Project", projects.find((p) => p.id === s()?.project)?.name)}
        {row("Last recorded event", "Tue, 27 Oct 11:43:08")}
        <div class="hint">Activity stays on {s()?.machine}. Closing this window does not stop your agent.</div>
      </section>
      <section class="insp-sec">
        <div class="eyebrow">Account</div>
        {row("Account", acc() ? `${acc()!.label} · ${acc()!.plan}` : undefined)}
        {row("Agent home", "~/.config/hgs/accounts/claude-a1", true)}
        {row("Model", `${s()?.model} ${s()?.effort ?? ""}`)}
      </section>
      <section class="insp-sec">
        <div class="eyebrow">Session usage</div>
        <table class="usage">
          <tbody>
            <For each={[["Total tokens", "1.84M"], ["Input (including cache)", "1.71M"], ["Uncached input", "142k"], ["Cache read", "1.52M"], ["Cache write", "48k"], ["Output", "126k"], ["Reasoning (in output)", "61k"], ["Recorded requests", "212"], ["Native cost estimate", "$18.40"], ["Tool time", "41 min"], ["Lines added / removed", "+1 204 / −388"], ["Latest context", "72k of 200k"]]}>
              {([k, v]) => <tr><td>{k}</td><td class="tabular">{v}</td></tr>}
            </For>
          </tbody>
        </table>
      </section>
      <div class="insp-foot"><span class="muted">Context: 36%</span><span class="grow" /><button class="btn small" onClick={() => setDialog("clear-session")}>Clear session…</button></div>
    </div>
  );
}

function Worktrees() {
  const s = () => selected();
  return (
    <div class="insp-body">
      <div class="insp-h"><b>Folder and worktrees</b><span class="grow" /><button class="icon-btn small" title="Refresh"><Icon name="refresh-cw" size={13} /></button></div>
      <div class="hint">Existing working folders on {s()?.machine}. Snapshot: 11:42</div>
      <ul class="wt-tree">
        <li class="wt-root">
          <button class="wt-head"><Icon name="folder-git-2" size={14} /><span><b>Main checkout</b> / main</span><span class="dim">(3 sessions)</span></button>
          <div class="mono dim wt-path">/home/example/src/aurora</div>
          <ul><li><button class="link" onClick={() => setSelectedKey("ws:4")}>accessibility (Working)</button></li><li><button class="link">search-index (Error)</button></li></ul>
        </li>
        <li class="wt-root on">
          <button class="wt-head"><Icon name="git-branch" size={14} /><span><b>Linked worktree</b> / feat/api-v2</span><span class="dim">(2 sessions)</span></button>
          <div class="mono dim wt-path">/home/example/src/aurora-work-api-v2</div>
          <ul><li><button class="link">api-refactor (Working)</button></li><li><button class="link">api-refactor-fork (Ready)</button></li></ul>
        </li>
        <li class="wt-root">
          <button class="wt-head"><Icon name="git-branch" size={14} /><span><b>Linked worktree</b> / chore/old-pager</span><span class="warn">(prunable)</span></button>
          <div class="mono dim wt-path">/home/example/src/aurora-work-old-pager</div>
        </li>
      </ul>
      <div class="insp-foot">
        <button class="btn small" onClick={() => toast("Filter", "List shows all sessions again")}>All sessions</button>
        <button class="btn small" onClick={() => setDialog("worktree-cleanup")}>Clean up (1)…</button>
        <span class="grow" />
        <button class="btn small" onClick={() => setDialog("new-session")}><Icon name="plus" size={12} />New session…</button>
      </div>
    </div>
  );
}

export function Inspector() {
  return (
    <aside class="inspector" aria-label="Side panel">
      <div class="insp-tabs tabs" role="tablist">
        <For each={TABS}>{(t) => (
          <button class="tab" role="tab" classList={{ on: inspectorTab() === t.id }} aria-selected={inspectorTab() === t.id} onClick={() => setInspectorTab(t.id)}>{t.label}{t.id === "tasks" ? <TaskTabBadge /> : null}</button>
        )}</For>
        <span class="grow" />
        <button class="icon-btn small" title="Collapse side panel (Ctrl+I)" onClick={() => setInspectorOpen(false)}><Icon name="panel-right-close" size={14} /></button>
      </div>
      <div class="insp-scroll">
        <Switch>
          <Match when={inspectorTab() === "plan"}><PlanAgents /></Match>
          <Match when={inspectorTab() === "details"}><Details /></Match>
          <Match when={inspectorTab() === "worktrees"}><Worktrees /></Match>
          <Match when={inspectorTab() === "tasks"}><TasksPanel /></Match>
        </Switch>
      </div>
    </aside>
  );
}
