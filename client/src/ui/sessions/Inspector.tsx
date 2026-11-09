import { For, Show, Switch, Match, Suspense, lazy, createEffect, createMemo, createSignal, onCleanup, onMount, on, type JSX } from "solid-js";
import { Portal, isServer } from "solid-js/web";
import { Icon } from "../lib/icons";
import { selected, setSubagentOpen, setDetailTab, setSelectedKey, subagentOpen, setMenu } from "../../store/ui";
import { inspectorOpen, inspectorTab, inspectorWidth, inspectorClosed, setInspectorOpen, setInspectorTab, setInspectorWidth, toggleInspectorCard, panelGeometry, inspectorHint, INSPECTOR_TABS, type InspectorTab } from "../../store/inspector";
import { followedFor, detailsFor, copyIdentity, copy, notify } from "../../store/actions";
import { ironRuleCounts } from "../../store/ironRuleCounts";
import { newSessionFromWorktree } from "./newsession-view";
import { KeepWarmRow } from "./KeepWarmRow";
import { catalogStatus, catalogRoots } from "./worktree-dialogs";
import { openWorktreeReview, worktreeReviewDialog } from "../../store/worktreeCleanup";
import { computerOffline, orderedRows } from "../../store/catalog";
import { groupOf, grouping, INBOX } from "../../store/organization";
import { sessionIdentity, snapshotOf } from "./actions-ui";
import { tasks } from "../../store/tasks";
import { taskProject } from "../../store/taskContext";
import "../styles/tasks.css";
import { object, text } from "../screens/tasks-ui";
import { cardKey, cardOpen, changesView, compactCount, contextUsage, diffOutcome, homeShort, pathParts, sessionAccount, sessionGoal, sessionPlan, sideTeam, usageRows, withRequest, type DiffOutcome } from "./inspector-data";
import { skin } from "../../skin/runtime";
import { selectedTeam, teams } from "../../store/teams";
import { agentKind } from "../../store/catalog";
import { AgentBadge } from "./SessionCard";
import { duration, memberStatus, stateLabel, stateTone } from "../teams/team-model";
import { roleStyle } from "../teams/roles";
import { machineCards } from "../../store/machines";
import { flag } from "../../store/settings";
import { SocratiCodePanel } from "./SocratiCodePanel";
import { SessionLeadPanel } from "./SessionLeadPanel";
import { privateStoreNotices } from "../../store/privateStoreNotices";
import { toast } from "../../store/notices";
import { filterFolder, clearFolderFilter } from "../../store/folderFilter";
import { of, sameKey, native } from "./paths";
import type { BoundRow } from "./scope-filters";
import type { Session } from "./types";
import { IronRulesPanel } from "./IronRulesPanel";
import { captionText, childStatus } from "./SessionCard";
import { currentChildren, finishedFoldId, foldedChildren, splitSubagents, workingChildren } from "./subagents";
import { tr, trNoop } from "../../i18n";
import { accountsPage } from "../../store/accounts";
import { openFilterAccounts } from "../../store/accountFilters";
import { sessionUsageView } from "./session-usage-ui";
import { composer } from "../../store/composer";
import * as bridge from "../../bridge";
import { eventTimeOf } from "../lib/dates";
import "../styles/inspector.css";

// Keep the always-visible badge eager; load the board only when its tab opens.
const TasksPanel = lazy(() => import("../screens/Tasks").then((module) => ({ default: module.TasksPanel })));
function TaskTabBadge() {
  // Each stage's total: a windowed list loads only some of its rows.
  const count = (stage: "review" | "todo") => tasks.project() === taskProject()?.id ? tasks.total(stage) : 0;
  return <Show when={taskProject() && tasks.loaded()}><span class="tk-tabbadge"><Show when={count("review")}>{(n) => <span class="tk-tb review">{n()}</span>}</Show><Show when={count("todo")}>{(n) => <span class="tk-tb todo">{n()}</span>}</Show></span></Show>;
}

const LABEL: Record<InspectorTab, string> = { plan: trNoop("Plan & agents"), details: trNoop("Details"), worktrees: trNoop("Worktrees"), tasks: trNoop("Tasks") };
const raw = () => selected() ? followedFor(selected()!.key).value : undefined;
const SHELLS = new Set(["sh", "shell", "bash", "zsh", "pwsh"]);
/** Computers whose account catalog the Inspector already asked for in this run. */
const askedCatalogs = new Set<string>();
function Card(props: { id: string; title: JSX.Element; aside?: JSX.Element; class?: string; classList?: Record<string, boolean>; onContextMenu?: JSX.EventHandler<HTMLElement, MouseEvent>; folded?: boolean; children: JSX.Element }) {
  const open = () => cardOpen(props.id, !props.folded, inspectorClosed());
  return <section class={`icard ${props.class ?? ""}`} classList={{ closed: !open(), ...props.classList }} onContextMenu={props.onContextMenu} data-section={props.id}><header class="icard-head">
    <button class="icard-toggle" aria-expanded={open()} aria-controls={`icard-${props.id}`} onClick={() => toggleInspectorCard(cardKey(props.id, !props.folded))}><span class="icard-title">{props.title}</span><Icon name="chevron-down" size={12} class="icard-chev" /></button>
    <Show when={props.aside}><span class="icard-aside">{props.aside}</span></Show>
  </header><Show when={open()}><div class="icard-body" id={`icard-${props.id}`}>{props.children}</div></Show></section>;
}
/**
 * A value row. With `copy` the whole row is a button: hover highlights it, the
 * copy mark shows at its right end, and a click copies that text (owner 2026-11-07).
 */
function Row(props: { icon: string; label?: string; mono?: boolean; title?: string; tail?: JSX.Element; copy?: string; onCopy?: () => void; copyName?: string; children: JSX.Element }) {
  const body = () => <><span class="irow-ico"><Icon name={props.icon} size={16} /></span><span class="irow-val ell" classList={{ mono: props.mono }}>{props.children}</span>{props.tail}<Show when={props.label}><span class="irow-label">{props.label}</span></Show></>;
  return <Show when={props.copy} fallback={<div class="irow" title={props.title}>{body()}</div>}>{(text) =>
    <button type="button" class="irow copyable" title={copyTip(props.title)} aria-label={props.copyName ?? (props.label ? tr("Copy %1: %2", props.label, text()) : tr("Copy"))} onClick={() => (props.onCopy ? props.onCopy() : void copyValue(text()))}>{body()}<CopyMark /></button>}</Show>;
}
const copyTip = (title?: string) => (title ? `${title}\n${tr("Click to copy")}` : tr("Click to copy"));
const CopyMark = () => <span class="copy-mark" aria-hidden="true"><Icon name="copy" size={13} /></span>;
// copy() reports a failure itself; confirm only success.
const copyValue = (text: string) => copy(text).then((out) => { if (out.outcome === "done") notify("info", tr("Copied")); });
/** Finished subagents shown per step once unfolded (zerus-5ffek). */
const FINISHED_PAGE = 50;
const sameIds = (a: readonly string[], b: readonly string[]) => a.length === b.length && a.every((id, i) => id === b[i]);
/**
 * A session's subagents (owner, 2026-11-08, T-ug5hxx): the working ones stay
 * in view and the rest fold behind their count, unfolding in steps, so a
 * session with many subagents renders and scrolls a bounded list. Rows follow
 * stable IDs (splitSubagents).
 */
function Subagents(props: { session: Session | undefined; online: boolean }) {
  const children = () => props.session?.subagents ?? [];
  const current = () => !!props.session && currentChildren(props.session, props.online);
  const byId = createMemo(() => new Map(children().map((child) => [child.id, child])));
  const split = createMemo(() => splitSubagents(children(), current(), subagentOpen()),
    undefined, { equals: (a, b) => sameIds(a.active, b.active) && sameIds(a.finished, b.finished) });
  // Folded again for each session.
  const [unfolded, setUnfolded] = createSignal(0);
  createEffect(on(() => props.session?.key, () => setUnfolded(0)));
  const finished = createMemo(() => split().finished.slice(0, unfolded()), undefined, { equals: sameIds });
  const row = (id: string) => <Show when={byId().get(id)}>{(child) =>
    <Row icon="circle" label={childStatus(child(), current()).caption} title={child().summary}><button class="link" onClick={() => { setSubagentOpen(id); setDetailTab("activity"); }}>{child().name} →</button><span class="dim"> {child().summary}</span></Row>}</Show>;
  return <Show when={children().length} fallback={<div class="hint">{tr("No subagent activity has been reported yet.")}</div>}>
    <div class="isub">
      <For each={split().active} fallback={<div class="hint">{tr("No subagent is working now.")}</div>}>{row}</For>
      <Show when={split().finished.length}>
        <button type="button" class="isub-more" aria-expanded={unfolded() > 0} onClick={() => setUnfolded(unfolded() ? 0 : FINISHED_PAGE)}>
          <Icon name={unfolded() ? "chevron-down" : "chevron-right"} size={12} />{tr("Finished: %1", split().finished.length)}
        </button>
        <For each={finished()}>{row}</For>
        <Show when={unfolded() && unfolded() < split().finished.length}>
          <button type="button" class="isub-more" onClick={() => setUnfolded(unfolded() + FINISHED_PAGE)}>{tr("Show %1 more", Math.min(FINISHED_PAGE, split().finished.length - unfolded()))}</button>
        </Show>
      </Show>
    </div>
  </Show>;
}
/** Task 262: the session's subagents, finished ones folded into "N finished, show" as in Qt; the fold is remembered per session. */
function SubagentRows(props: { session: Session; online: boolean; summaries?: boolean; empty?: JSX.Element }) {
  const current = () => currentChildren(props.session, props.online);
  const id = () => finishedFoldId(props.session.key);
  const open = () => cardOpen(id(), false, inspectorClosed());
  const fold = createMemo(() => foldedChildren((props.session.subagents ?? []).slice(0, 100), current(), open()));
  return <>
    <For each={fold().shown} fallback={fold().finished ? undefined : props.empty}>{(child) => <Row icon="circle" label={childStatus(child, current()).caption} title={child.summary}><button class="link" onClick={() => { setSubagentOpen(child.id); setDetailTab("activity"); }}>{child.name} →</button><Show when={props.summaries}><span class="dim"> {child.summary}</span></Show></Row>}</For>
    <Show when={fold().finished || (open() && current() && (props.session.subagents ?? []).some((child) => child.state !== "working" && child.state !== "attention" && !child.group))}>
      <div class="side-finished" data-finished={fold().finished}>
        <Show when={!open()} fallback={<button class="link" onClick={() => toggleInspectorCard(cardKey(id(), false))}>{tr("hide finished")}</button>}>
          <span class="dim">{fold().finished === 1 ? tr("1 finished") : tr("%1 finished", fold().finished)}</span>
          <button class="link" onClick={() => toggleInspectorCard(cardKey(id(), false))}>{tr("show")}</button>
        </Show>
      </div>
    </Show>
  </>;
}

function PlanAgents() {
  const s = selected, plan = createMemo(() => sessionPlan(raw())), goal = createMemo(() => sessionGoal(raw()));
  const online = () => !!s() && !computerOffline(s()!.computer);
  return <div class="insp-body">
    <Card id="plan-main" title={tr("Main agent")} aside={<span class="accent">{s() ? captionText(s()!.caption) : tr("Not reported")}</span>}><Row icon="activity" label={tr("status")}>{s() ? captionText(s()!.caption) : tr("Not reported")}</Row><Row icon="terminal" label={tr("action")} mono title={s()?.summary}>{s()?.summary || tr("Not reported")}</Row></Card>
    <Show when={goal().objective}><Card id="plan-goal" title={tr("Goal")} aside={<span class="muted">{goal().status === "active" ? tr("Pursuing goal") : goal().status}</span>}><p class="icard-text">{goal().objective}</p><Show when={goal().completion}><div class="hint">{tr("Reported by the agent. Completion: %1", goal().completion!)}</div></Show></Card></Show>
    <Show when={plan().length}><Card id="plan-tasks" title={tr("Tasks")} aside={<span class="dim">{tr("%1 of %2 completed (last recorded)", plan().filter((p) => p.status === "completed").length, plan().length)}</span>}><For each={plan()}>{(p) => <Row icon={p.status === "completed" ? "check" : "circle-dot"} label={p.status === "completed" ? tr("Done") : p.status === "in_progress" ? tr("In progress") : tr("To do")}>{p.title}</Row>}</For></Card></Show>
    <Card id="plan-agents" title={tr("Subagents")} aside={<span class="dim">{tr("%1 active / %2 total", s() ? workingChildren(s()!, online()) : 0, s()?.subagents?.length ?? 0)}</span>}>
      <Subagents session={s()} online={online()} />
    </Card>
  </div>;
}
/** Task 68: the selected session's project and own iron rules (Qt's Details card). */
function IronRulesCard(props: { session: Session; id?: string; folded?: boolean }) {
  const [count, setCount] = createSignal(0);
  const project = () => { const group = groupOf(props.session.key); return group === INBOX ? "" : group ?? ""; };
  const context = () => ({
    computer: props.session.computer,
    project: project(),
    session: props.session.runId ? { name: props.session.name, run_id: props.session.runId, conversation_id: props.session.conversationId ?? "" } : undefined,
  });
  // The composer's iron rules chip shows the same count (Qt's changed callback).
  const counted = (n: number) => { setCount(n); ironRuleCounts.set(props.session.key, n); };
  return <Card id={props.id ?? "details-rules"} folded={props.folded} title={tr("Iron rules")} aside={<Show when={count()}><span class="dim tabular">{count()}</span></Show>}>
    <IronRulesPanel context={context()} onCount={counted} />
  </Card>;
}

/** Open diff's dialog (Qt showDiff): non-modal, 820 by 600, the diff read-only, unwrapped and monospaced, and Close. */
function DiffDialog(props: { diff: Extract<DiffOutcome, { title: string }>; onClose: () => void }) {
  let dialog!: HTMLDialogElement;
  onMount(() => { dialog.show(); dialog.querySelector<HTMLElement>(".diff-text")?.focus(); });
  return <Portal><dialog ref={dialog} class="dlg diff-dlg" aria-labelledby="diff-dlg-title" onClose={() => props.onClose()}
    onKeyDown={(e) => { if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); dialog.close(); } }}>
    <div class="dlg-head"><h2 id="diff-dlg-title" class="ell" title={props.diff.title}>{props.diff.title}</h2></div>
    <div class="dlg-body diff-body">
      <textarea class="diff-text mono" readOnly wrap="off" spellcheck={false} aria-label={props.diff.title} value={props.diff.text} />
      <Show when={props.diff.note}><div class="hint">{props.diff.note}</div></Show>
    </div>
    <div class="dlg-foot"><span class="spacer" /><button type="button" class="btn primary" onClick={() => dialog.close()}>{tr("Close")}</button></div>
  </dialog></Portal>;
}

/**
 * Qt parity (I3): the Changes card (SessionsWindow.cpp:1680-1707,
 * 7503-7563), folded by default. The folder is the inspected one, else the
 * session's; its status is read when the session changes, when the card
 * opens and every 15 s while it is open (Rust reads a folder at most every
 * 15 s). Commit changes… only puts a request into the composer; Open diff
 * reads the diff into a dialog.
 */
function ChangesCard(props: { session: Session; id: string }) {
  const online = () => !computerOffline(props.session.computer);
  const target = createMemo(() => {
    const cwd = detailsFor(props.session, online(), followedFor(props.session.key))?.cwd;
    const path = typeof cwd === "string" ? cwd : props.session.cwdPath ?? "";
    return { computer: props.session.computer, path, key: `${props.session.computer}\n${path}` };
  }, undefined, { equals: (a, b) => a.key === b.key });
  const [status, setStatus] = createSignal<unknown>();
  const [diff, setDiff] = createSignal<Extract<DiffOutcome, { title: string }>>();
  const [diffing, setDiffing] = createSignal(false);
  const open = () => cardOpen(props.id, false, inspectorClosed());
  let ticket = 0, disposed = false;
  const read = async () => {
    const at = target();
    if (!at.path || computerOffline(at.computer)) return;
    const mine = ++ticket;
    let value: unknown;
    try { value = await bridge.gitStatus(at.computer, at.path); } catch { value = { state: "unavailable" }; }
    if (!disposed && mine === ticket && at.key === target().key) setStatus(value);
  };
  createEffect(on(() => target().key, () => { ++ticket; setStatus(undefined); void read(); }));
  createEffect(on(open, (now) => { if (now) void read(); }, { defer: true }));
  if (!isServer) { const timer = setInterval(() => { if (open()) void read(); }, 15_000); onCleanup(() => clearInterval(timer)); }
  onCleanup(() => { disposed = true; ++ticket; });
  const view = createMemo(() => changesView(status()));
  const canDiff = () => view().known && !view().clean;
  const canCommit = () => canDiff() && !subagentOpen();
  const commit = () => {
    if (!canCommit()) return;
    const next = withRequest(composer.text(), tr("Commit the current changes with a short message."));
    composer.input(next, next.length, next.length);
    const field = document.querySelector<HTMLTextAreaElement>(".cmp-editor");
    field?.focus();
    try { field?.setSelectionRange(next.length, next.length); } catch { /* not rendered */ }
  };
  const openDiff = async () => {
    const at = target();
    if (!canDiff() || diffing() || !at.path) return;
    setDiffing(true);
    let outcome: DiffOutcome;
    try { outcome = diffOutcome(await bridge.gitDiff(at.computer, at.path), ""); }
    catch (error) { outcome = diffOutcome({}, typeof error === "string" ? error : error instanceof Error ? error.message : String(error)); }
    finally { setDiffing(false); }
    if ("notice" in outcome) notify("error", outcome.notice, true);
    else if (!disposed) setDiff(outcome);
  };
  return <>
    <Card id={props.id} folded title={tr("Changes")} aside={<Show when={view().aside}><span class="dim tabular">{view().aside}</span></Show>}>
      <Show when={view().rows.length || view().more > 0}><div class="chg-list">
        <For each={view().rows}>{(row) => { const [head, tail] = pathParts(row.path); return <div class="chg-row" title={row.path}>
          <span class="chg-path mono"><span class="chg-head">{head}</span><span class="chg-tail">{tail}</span></span>
          <span class="chg-added accent tabular">{row.added}</span><span class="chg-deleted danger tabular">{row.deleted}</span>
        </div>; }}</For>
        <Show when={view().more > 0}><div class="hint">{tr("%1 more files", view().more)}</div></Show>
      </div></Show>
      <Show when={view().empty}><div class="hint">{view().empty}</div></Show>
      <div class="side-actions">
        <button type="button" class="btn small" disabled={!canCommit()} title={tr("Ask this session's agent to commit: the request goes into the composer, not sent")} onClick={commit}>{tr("Commit changes…")}</button>
        <button type="button" class="btn small ghost" disabled={!canDiff() || diffing()} onClick={() => void openDiff()}>{tr("Open diff")}</button>
      </div>
    </Card>
    <Show when={diff()}>{(shown) => <DiffDialog diff={shown()} onClose={() => setDiff(undefined)} />}</Show>
  </>;
}

function useDetails() {
  const s = () => selected()!;
  const details = () => detailsFor(s(), !computerOffline(s().computer), followedFor(s().key));
  createEffect(() => {
    const view = details();
    privateStoreNotices((notice) => toast(notice.title, notice.body, "error", true))
      .observe(s().computer, s().machine, !computerOffline(s().computer), view?.private_stores ?? []);
  });
  const identity = () => sessionIdentity(s(), details());
  const value = (v: string | undefined) => v || tr("Not reported");
  const rows = createMemo(() => usageRows(raw()));
  const usage = () => sessionUsageView(object(raw()).session_usage);
  const context = () => composer.view()?.context;
  const clear = () => composer.view()?.clear;
  // The listing's recorded binding, else the inspect's (account_id/account_home).
  const bound = createMemo((): BoundRow & { accountId: string; accountHome: string } => {
    const row = s(), v = object(raw());
    return { computer: row.computer, cmd: row.cmd, accountProvider: row.accountProvider, accountReported: row.accountReported, accountId: row.accountId || text(v.account_id), accountHome: row.accountHome || text(v.account_home) };
  }, undefined, { equals: (a, b) => (Object.keys(a) as (keyof BoundRow)[]).every((k) => a[k] === b[k]) });
  const account = createMemo(() => sessionAccount(bound(), accountsPage()));
  // Resolve against the screens hub's catalogs; read a computer's catalog once if none is held.
  createEffect(on(() => s().computer, (computer) => { if (!account().account && !askedCatalogs.has(computer)) { askedCatalogs.add(computer); void openFilterAccounts(); } }));
  const accountText = () => {
    const a = account().account, plan = a?.usage.identity.plan;
    if (a) return plan ? tr("%1 (%2)", a.label || tr("Unnamed account"), plan) : a.label || tr("Unnamed account");
    return bound().accountId || bound().accountHome || (SHELLS.has(s().cmd) ? tr("No account (shell)") : tr("Account not recorded"));
  };
  // The account as the row names it, without the plan suffix: its sign-in email when known, else its
  // label, else the recorded ID or home. A shell or an unrecorded account has nothing to copy.
  const accountCopy = () => {
    const a = account().account;
    return a?.usage.identity.email || a?.label || bound().accountId || bound().accountHome || undefined;
  };
  const home = () => bound().accountHome || account().profile?.home || "";
  const profileId = () => bound().accountId || account().profile?.id || "";
  return { s, details, identity, value, rows, usage, context, clear, account, accountText, accountCopy, home, profileId };
}
function Details() {
  const { s, details, identity, value, rows, usage, context, clear, account, accountText, accountCopy, home, profileId } = useDetails();
  return <div class="insp-body"><Card id="details-id" title={tr(identity().title)}><Row icon="key-round" mono title={tr(identity().title)} copy={identity().value || undefined} copyName={tr("Copy %1: %2", tr(identity().title), identity().value)} onCopy={() => void copyIdentity(snapshotOf(s()), details())}>{value(identity().value)}</Row></Card>
    <Card id="details-session" title={tr("Session")}><Row icon="message-square" label={tr("name")} mono title={s().name} copy={s().name}>{s().name}</Row><Row icon="monitor" label={tr("machine")} copy={s().machine}>{s().machine}</Row><Row icon="folder" label={tr("directory")} mono title={s().cwdPath} copy={s().cwdPath}>{value(s().cwdPath)}</Row><Row icon="git-branch" label={tr("branch")} copy={s().branch}>{value(s().branch)}</Row><Row icon="layers" label={tr("project")} copy={grouping().headers.find((p) => p.id === groupOf(s().key))?.name}>{grouping().headers.find((p) => p.id === groupOf(s().key))?.name ?? tr("Not reported")}</Row><Row icon="clock" label={tr("last event")}>{s().updatedAt ? eventTimeOf(new Date(s().updatedAt! * 1000)) : tr("Not reported")}</Row><Show when={s().state !== "archived"}><KeepWarmRow session={s()} inspected={raw()} online={!computerOffline(s().computer)} /></Show></Card>
    <IronRulesCard session={s()} />
    <ChangesCard session={s()} id="changes" />
    <Card id="details-account" title={tr("Account")}><Row icon="user-round-cog" label={tr("account")} title={profileId() ? tr("Profile %1", profileId()) : undefined} copy={accountCopy()} tail={<Show when={account().tag}><span class="acct-pid tabular" title={tr("Profile %1", profileId())}>{account().tag}</span></Show>}>{accountText()}</Row><Row icon="hard-drive" label={tr("agent home")} mono title={home() || undefined} copy={home() || undefined}>{value(homeShort(home()))}</Row><Row icon="sparkles" label={tr("model")} copy={s().model || undefined}>{value(s().model)} {s().effort}</Row></Card>
    <Card id="details-lead" title={tr("Session lead")}><SessionLeadPanel /></Card>
    <Card id="details-usage" title={tr("Session usage")}><Show when={rows().length} fallback={<div class="hint">{tr("No native usage reported for this conversation yet.")}</div>}><table class="usage"><tbody><For each={rows()}>{([label, val]) => <Show when={val !== tr("Not reported")} fallback={<tr><td>{label}</td><td class="tabular">{val}</td></tr>}>
      <tr class="copyable" tabindex="0" role="button" title={tr("Click to copy")} aria-label={tr("Copy %1: %2", label, val)} onClick={() => void copyValue(val)}
        onKeyDown={(e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); void copyValue(val); } }}><td>{label}</td><td class="tabular"><span class="u-val">{val}<CopyMark /></span></td></tr></Show>}</For></tbody></table><Show when={usage().costNote}><div class="hint">{usage().costNote}</div></Show><div class="hint">{usage().footer}</div></Show></Card>
    <Show when={flag("extensions/socraticode") === true && !!s().cwdPath}><Card id="details-socraticode" title={tr("SocratiCode")}><SocratiCodePanel /></Card></Show>
    <Show when={!subagentOpen()}><div class="insp-foot"><span class="muted" title={context()?.tooltip}>{tr("Context: %1", context()?.text || tr("Not reported"))}</span><span class="grow" /><button class="btn small" title={clear()?.reason || undefined} disabled={!clear()?.enabled} onClick={() => composer.openClear()}>{clear()?.running ? tr("Clearing…") : tr("Clear session…")}</button></div></Show>
  </div>;
}
function Worktrees() {
  const [catalog, setCatalog] = createSignal<Record<string, unknown>>({}), [reading, setReading] = createSignal(false);
  const [picked, setPicked] = createSignal<string>();
  const [catalogError, setCatalogError] = createSignal("");
  const context = createMemo(() => { const s = selected(); return s?.cwdPath ? { computer: s.computer, path: s.cwdPath, key: `${s.computer}\n${s.cwdPath}` } : undefined; });
  let wanted = 0, epoch = 0, answered = false, busy = false, again = false, disposed = false;
  let returnFocus: HTMLElement | undefined, cleanupButton: HTMLButtonElement | undefined;
  const read = async (refresh = false) => {
    if (busy) { again = true; return; }
    const target = context(); if (!target || computerOffline(target.computer)) return;
    busy = true; setReading(true); setCatalogError(""); const ticket = ++wanted;
    try { const value = await bridge.worktreeCatalog(target.computer, target.path, refresh); if (!disposed && ticket === wanted && target.key === context()?.key) { answered = true; setCatalog(value); } }
    catch (error) { if (!disposed && target.key === context()?.key) setCatalogError(String(error)); }
    finally { busy = false; if (!disposed) { setReading(false); if (again) { again = false; void read(); } else if (returnFocus) { const active = document.activeElement, tab = document.querySelector(".inspector .insp-tabs [aria-selected=true]"); if (active === document.body || active === tab) (returnFocus.isConnected ? returnFocus : cleanupButton)?.focus(); returnFocus = undefined; } } }
  };
  createEffect(on(() => context()?.key, () => { ++wanted; const generation = ++epoch; answered = false; setCatalog({}); setCatalogError(""); setPicked(undefined); setMenu(undefined); const target = context(); if (target) void bridge.worktreeSnapshot(target.computer, target.path).then((value) => { if (!disposed && generation === epoch && !answered) setCatalog(value); }).catch(() => {}); void read(); }));
  createEffect(on(worktreeReviewDialog, (dialog, previous) => { if (!dialog && previous) { returnFocus = previous.origin; void read(true); } }));
  onCleanup(() => { disposed = true; ++wanted; setMenu(undefined); });
  const style = () => of(machineCards[""]?.platform, selected()?.computer === "", /Win/.test(navigator.platform) ? "windows" : "posix");
  const trees = createMemo(() => catalogRoots(catalog(), context()?.path ?? ""));
  const current = () => trees().find((t) => text(t.path) === picked()) ?? trees().find((t) => text(t.path) === text(catalog().selected_root)) ?? trees()[0];
  const online = () => !!context() && !computerOffline(context()!.computer);
  createEffect(on(online, (connected, previous) => { if (connected && previous === false) void read(); }));
  const fresh = () => online() && !reading() && !catalogError() && catalog().stale !== true && ["ok", "not_repo"].includes(text(catalog().state));
  const canCleanup = () => fresh() && catalog().state === "ok" && !!text(catalog().common_dir) && trees().some((t) => t.kind === "linked");
  const review = (focus: string, origin?: HTMLElement) => { const target = context(); if (target && canCleanup()) void openWorktreeReview(target.computer, target.path, focus, origin); };
  const menu = (event: MouseEvent & { currentTarget: HTMLElement }, tree: Record<string, unknown>) => {
    event.preventDefault(); const path = text(tree.path), target = context(); setPicked(path);
    const origin = event.currentTarget.querySelector<HTMLElement>(".wt-path") ?? undefined;
    setMenu({ x: event.clientX, y: event.clientY, items: [
      ...(tree.kind === "linked" ? [{ label: tree.available === false ? tr("Forget missing worktree…") : tr("Remove worktree…"), icon: "trash", danger: true, disabled: !canCleanup(), hint: tr("Review this worktree on its computer before anything is removed."), action: () => { if (target?.key === context()?.key) review(path, origin); } }, { separator: true as const }] : []),
      { label: tr("Copy path"), icon: "copy", action: () => { void copy(path); } },
    ] });
  };
  // One catalog pass, retaining at most fifty shown sessions per displayed folder/checkout.
  const sessions = createMemo(() => {
    const roots = new Map<string, Session[]>(), folders = new Map<string, Session[]>();
    for (const tree of trees()) (tree.kind === "folder" ? folders : roots).set(sameKey(text(tree.path), style()), []);
    for (const s of orderedRows()) {
      if (s.computer !== selected()?.computer) continue;
      const root = roots.get(sameKey(s.gitRoot, style())), folder = folders.get(sameKey(s.cwdCanonical, style()));
      if (root && root.length < 50) root.push(s);
      if (folder && folder.length < 50) folder.push(s);
    }
    return (tree: Record<string, unknown>) => (tree.kind === "folder" ? folders : roots).get(sameKey(text(tree.path), style())) ?? [];
  });
  return <div class="insp-body"><div class="insp-h"><b>{tr("Folder and worktrees")}</b><span class="grow" /><button class="icon-btn small" aria-label={tr("Refresh")} title={tr("Refresh")} disabled={reading() || !online()} onClick={() => void read(true)}><Icon name="refresh-cw" size={13} /></button></div>
    <div class="hint" style={{ "white-space": "pre-line" }} classList={{ warn: !online() || !!catalogError() || catalog().partial === true || catalog().stale === true, danger: catalog().state === "folder_unavailable" }}>{catalogStatus(context()?.path ?? "", context()?.computer || machineCards[""]?.host || "this computer", catalog(), online(), reading(), catalogError()).replace(/\.?\nSnapshot:/u, ". Snapshot:")}</div>
    <For each={trees()}>{(tree) => <Card id={`wt-${text(tree.path)}`} class="wt" onContextMenu={(e) => menu(e, tree)} classList={{ on: text(tree.path) === text(current()?.path), gone: tree.available === false }} title={<><b>{tree.kind === "folder" ? tr("Folder") : tree.kind === "main" ? tr("Main checkout") : tree.kind === "bare" ? tr("Bare repository") : tr("Linked worktree")}</b><Show when={text(tree.branch)}><span class="wt-branch"> / {text(tree.branch)}</span></Show><Show when={tree.detached === true}><span class="wt-branch"> / {tr("detached %1", text(tree.head).slice(0, 8))}</span></Show></>} aside={<><Show when={tree.locked === true}><span class="dim">{tr("(locked)")}</span></Show><Show when={tree.prunable === true}><span class="warn">{tr("(prunable)")}</span></Show><Show when={tree.available === false}><span class="danger">{tr("(unavailable)")}</span></Show><Show when={sessions()(tree).length}><span class="dim">{sessions()(tree).length === 1 ? tr("(1 session)") : tr("(%1 sessions)", sessions()(tree).length)}</span></Show></>}>
      <button class="irow wt-path" title={tr("Show the sessions in this folder")} onClick={() => { const host = selected()?.computer; setPicked(text(tree.path)); if (host !== undefined) filterFolder({ computer: host, path: text(tree.path), checkout: tree.kind !== "folder", style: style() }); }}>
        <span class="irow-ico"><Icon name={tree.kind === "folder" ? "folder" : tree.kind === "main" ? "folder-git-2" : tree.kind === "bare" ? "box" : "git-branch"} size={16} stroke={1.75} /></span><span class="irow-val ell mono">{native(text(tree.path), style())}</span><span class="irow-label">{tr("folder")}</span>
      </button>
      <For each={sessions()(tree)}>{(s) => <Row icon="corner-down-right" label={captionText(s.caption)}><button class="link" onClick={() => { setSelectedKey(s.key); setDetailTab("activity"); }}>{s.title}</button></Row>}</For>
    </Card>}</For>
    <div class="insp-foot"><button class="btn small" onClick={() => { setPicked(undefined); clearFolderFilter(); }}>{tr("All sessions")}</button><button ref={cleanupButton} class="btn small" disabled={!canCleanup()} title={tr("Review this repository's worktrees on its computer before anything is removed")} onClick={(e) => review("", e.currentTarget)}>{tr("Clean up…")}</button><span class="grow" /><button class="btn small" disabled={!fresh() || !current() || current()?.available === false || current()?.kind === "bare"} onClick={() => { const s = selected(), path = text(current()?.path); if (s && path && fresh()) void newSessionFromWorktree(groupOf(s.key), s.computer, path); }}><Icon name="plus" size={12} />{tr("New session…")}</button></div>
  </div>;
}
/** Task 206: the Team section, Qt TeamPanel's running summary. */
function SideTeamCard() {
  const summary = () => selectedTeam();
  const view = () => { const row = selected(), id = summary()?.id; return row && id ? teams.team(row.computer, id) : undefined; };
  const [now, setNow] = createSignal(Date.now() / 1000);
  if (!isServer) { const timer = setInterval(() => setNow(Date.now() / 1000), 60_000); onCleanup(() => clearInterval(timer)); }
  const card = createMemo(() => { const team = view(); return team ? sideTeam(team, now()) : undefined; });
  const state = () => view()?.status ?? summary()?.status ?? "";
  const pending = () => view()?.pending_phase ?? summary()?.pending_phase ?? null;
  const members = () => view()?.members.filter((member) => member.state !== "removed") ?? [];
  const bar = (done: number, total: number) => <span class="side-bar" role="presentation"><i style={{ width: `${total ? Math.round(100 * done / total) : 0}%` }} /></span>;
  return <div class="side-team">
    <div class="side-team-top"><Icon name="users" size={15} /><b class="ell">{summary()?.name}</b><span class="grow" /><span class={`side-chip tone-${stateTone(state())}`}>{stateLabel(state())}</span></div>
    <Show when={pending()}>{(n) => <div><span class="side-chip tone-warn">{tr("phase %1 pending approval", n())}</span></div>}</Show>
    <Show when={card()}>{(c) => <>
      <Show when={c().phase}>{(phase) => <div class="side-progress"><span>{tr("Phase %1", phase().n)}</span>{bar(phase().done, phase().total)}<span class="tabular dim">{tr("%1/%2", phase().done, phase().total)}</span></div>}</Show>
      <Show when={c().phases.total}><div class="side-progress"><span>{tr("Phases")}</span>{bar(c().phases.done, c().phases.total)}<span class="tabular dim">{tr("%1/%2", c().phases.done, c().phases.total)}</span></div></Show>
      <div class="dim">{c().age ? tr("%1/%2 accepted, %3", c().accepted, c().total, duration(c().age)) : tr("%1/%2 accepted", c().accepted, c().total)}</div>
    </>}</Show>
    <button class="btn small primary" onClick={() => setDetailTab("team")}>{tr("Open Team")}</button>
    <Show when={members().length}>
      <div class="irow side-members-head"><span class="irow-ico" /><b>{tr("Members")}</b><span class="grow" /><span class="irow-label tabular dim">{members().length}</span></div>
      <For each={members()}>{(member) => { const status = () => memberStatus(view()!, member); return <div class="side-member" data-member={member.id}>
        <span class="role-badge ell" style={roleStyle(member.role, member.lead)}>{member.role || member.id}</span><AgentBadge agent={agentKind(member.agent)} narrow /><span class="grow" /><span class={`dim ell tone-${status().tone}`} title={status().text}>{status().short || status().text}</span>
      </div>; }}</For>
    </Show>
    <div class="hint">{tr("Plan, graph and member changes live in the Team tab.")}</div>
  </div>;
}

/** Task 206: the Compact skins' side panel, Qt's InspectorCards: one scroll of collapsible sections. */
function Sections() {
  const { s, details, identity, value, rows, usage, context, clear, accountText, accountCopy, home } = useDetails();
  const plan = createMemo(() => sessionPlan(raw())), goal = createMemo(() => sessionGoal(raw()));
  const online = () => !computerOffline(s().computer);
  const window = createMemo(() => contextUsage(raw()));
  return <div class="insp-body insp-sections">
    <Card id="sec-environment" title={tr("Environment")}>
      <Row icon="key-round" mono title={tr(identity().title)} copy={identity().value || undefined} copyName={tr("Copy %1: %2", tr(identity().title), identity().value)} onCopy={() => void copyIdentity(snapshotOf(s()), details())}>{value(identity().value)}</Row>
      <Row icon="monitor" label={tr("machine")} copy={s().machine}>{s().machine}</Row>
      <Row icon="folder" label={tr("directory")} mono title={s().cwdPath} copy={s().cwdPath}>{value(s().cwdPath)}</Row>
      <Row icon="git-branch" label={tr("branch")} copy={s().branch}>{value(s().branch)}</Row>
      <Row icon="sparkles" label={tr("model")} copy={s().model || undefined}>{value(s().model)} {s().effort}</Row>
      <Row icon="clock" label={tr("last event")}>{s().updatedAt ? eventTimeOf(new Date(s().updatedAt! * 1000)) : tr("Not reported")}</Row>
      <Show when={s().state !== "archived"}><KeepWarmRow session={s()} inspected={raw()} online={online()} /></Show>
    </Card>
    <Card id="sec-context" title={tr("Context")} aside={<Show when={window() && window()!.percent >= 0}><span class="dim tabular">{tr("%1%", window()!.percent)}</span></Show>}>
      <Show when={window()} fallback={<div class="hint">{tr("Context: %1", context()?.text || tr("Not reported"))}</div>}>{(w) => <>
        <div class="side-progress"><span>{w().limit ? tr("%1% used", w().percent) : tr("%1 used", compactCount(w().used))}</span>{w().limit ? <span class="side-bar" role="presentation"><i style={{ width: `${Math.min(100, w().percent)}%` }} /></span> : <span class="grow" />}<span class="tabular dim">{w().limit ? tr("%1/%2", compactCount(w().used), compactCount(w().limit)) : ""}</span></div>
      </>}</Show>
      {/* Qt: "128k left in the window" with Clear session beside it, then the account and the session's usage. */}
      <div class="side-actions"><span class="hint grow">{window()?.limit ? tr("%1 left in the window", compactCount(Math.max(0, window()!.limit - window()!.used))) : ""}</span><Show when={!subagentOpen()}><button class="btn small" title={clear()?.reason || undefined} disabled={!clear()?.enabled} onClick={() => composer.openClear()}>{clear()?.running ? tr("Clearing…") : tr("Clear session…")}</button></Show></div>
      <Row icon="user-round-cog" label={tr("account")} copy={accountCopy()}>{accountText()}</Row>
      <Row icon="hard-drive" label={tr("agent home")} mono title={home() || undefined} copy={home() || undefined}>{value(homeShort(home()))}</Row>
      <Show when={rows().length}><table class="usage"><tbody><For each={rows()}>{([label, val]) => <tr><td>{label}</td><td class="tabular">{val}</td></tr>}</For></tbody></table><Show when={usage().costNote}><div class="hint">{usage().costNote}</div></Show></Show>
    </Card>
    <Show when={selectedTeam()}><Card id="sec-team" title={tr("Team")}><SideTeamCard /></Card></Show>
    <Card id="sec-tasks" title={tr("Tasks")} aside={<Show when={plan().length}><span class="dim">{tr("%1 of %2 completed (last recorded)", plan().filter((p) => p.status === "completed").length, plan().length)}</span></Show>}>
      <Row icon="activity" label={tr("status")}>{captionText(s().caption)}</Row>
      <Row icon="terminal" label={tr("action")} mono title={s().summary}>{s().summary || tr("Not reported")}</Row>
      <Show when={goal().objective}><p class="icard-text">{goal().objective}</p></Show>
      <For each={plan()}>{(p) => <Row icon={p.status === "completed" ? "check" : "circle-dot"} label={p.status === "completed" ? tr("Done") : p.status === "in_progress" ? tr("In progress") : tr("To do")}>{p.title}</Row>}</For>
      <div class="irow side-members-head"><span class="irow-ico" /><b>{tr("Subagents")}</b><span class="grow" /><span class="irow-label tabular dim">{tr("%1 active / %2 total", workingChildren(s(), online()), s().subagents?.length ?? 0)}</span></div>
      <SubagentRows session={s()} online={online()} />
      <SessionLeadPanel />
    </Card>
    <IronRulesCard session={s()} id="sec-rules" folded />
    <Card id="sec-worktrees" title={tr("Worktrees")} folded><Worktrees /></Card>
    <ChangesCard session={s()} id="sec-changes" />
    <Card id="sec-projecttasks" title={tr("Project tasks")} folded aside={<TaskTabBadge />}><div class="side-tasks"><Suspense><TasksPanel /></Suspense></div></Card>
    <Show when={flag("extensions/socraticode") === true && !!s().cwdPath}><Card id="sec-socraticode" title={tr("SocratiCode")}><SocratiCodePanel /></Card></Show>
  </div>;
}

export function Inspector(props: { available: number }) {
  const sections = () => skin().layout["side-panel"] === "sections";
  const geometry = () => panelGeometry(props.available, inspectorWidth());
  let panel!: HTMLElement;
  let returnFocus: HTMLElement | undefined;
  const restoreFocus = () => queueMicrotask(() => { if (returnFocus?.isConnected && returnFocus !== document.body) returnFocus.focus(); else document.querySelector<HTMLButtonElement>("[data-cmd='show-side-panel']")?.focus(); });
  createEffect(on(inspectorOpen, (open, previous) => {
    if (open) returnFocus = document.activeElement instanceof HTMLElement ? document.activeElement : undefined;
    else if (previous) restoreFocus();
  }));
  const close = () => setInspectorOpen(false);
  createEffect(() => {
    if (!geometry().floating || !inspectorOpen()) return;
    const escape = (e: KeyboardEvent) => { if (e.key === "Escape" && !document.querySelector("dialog[open], [data-shortcut-context='none']")) { e.preventDefault(); close(); } };
    window.addEventListener("keydown", escape); onCleanup(() => window.removeEventListener("keydown", escape));
  });
  const resize = (e: PointerEvent) => {
    if (e.button !== 0) return; const el = e.currentTarget as HTMLElement; el.setPointerCapture(e.pointerId);
    const x = e.clientX, start = geometry().width;
    const move = (v: PointerEvent) => setInspectorWidth(start + x - v.clientX);
    const end = () => { el.removeEventListener("pointermove", move); el.removeEventListener("pointerup", end); el.removeEventListener("pointercancel", end); };
    el.addEventListener("pointermove", move); el.addEventListener("pointerup", end); el.addEventListener("pointercancel", end);
  };
  return <Show when={inspectorOpen()}><aside ref={panel} class="inspector" classList={{ floating: geometry().floating }} style={{ width: `${geometry().width}px` }} aria-label={tr("Side panel")}>
    <div class="insp-resize" tabindex="0" role="separator" aria-orientation="vertical" aria-label={tr("Side panel width")} aria-valuemin={240} aria-valuemax={900} aria-valuenow={inspectorWidth()} onPointerDown={resize} onDblClick={() => setInspectorWidth(400)} onKeyDown={(e) => { if (["ArrowLeft", "ArrowRight", "Home"].includes(e.key)) { e.preventDefault(); setInspectorWidth(e.key === "Home" ? 400 : inspectorWidth() + (e.key === "ArrowLeft" ? 20 : -20)); } }}><i /></div>
    {/* Task 354: the Compact panel has no 'Session' caption and no outer frame; the only thing above its sections is the collapse button. */}
    <Show when={sections()}><div class="insp-head"><span class="grow" /><button class="icon-btn small" aria-label={tr("Collapse side panel")} data-tip={inspectorHint(tr("Collapse side panel"))} data-tip-pos="left" onClick={close}><Icon name="panel-right-close" size={14} /></button></div>
      <div class="insp-scroll sections"><Show when={selected()} fallback={<div class="insp-body"><div class="hint">{tr("No session selected")}</div></div>}><Sections /></Show></div></Show>
    <Show when={!sections()}><div class="insp-tabs tabs" role="tablist" aria-label={tr("Side panel tabs")}><For each={INSPECTOR_TABS}>{(tab) => <button class="tab" role="tab" classList={{ on: inspectorTab() === tab }} title={tr(LABEL[tab])} aria-selected={inspectorTab() === tab} tabindex={inspectorTab() === tab ? 0 : -1} onClick={() => setInspectorTab(tab)} onKeyDown={(e) => { const i = INSPECTOR_TABS.indexOf(tab), next = e.key === "ArrowRight" ? (i + 1) % 4 : e.key === "ArrowLeft" ? (i + 3) % 4 : e.key === "Home" ? 0 : e.key === "End" ? 3 : -1; if (next >= 0) { e.preventDefault(); setInspectorTab(INSPECTOR_TABS[next]); panel.querySelectorAll<HTMLButtonElement>(".insp-tabs .tab")[next]?.focus(); } }}><span class="tab-label">{tr(LABEL[tab])}</span>{tab === "tasks" ? <TaskTabBadge /> : null}</button>}</For><span class="grow" /><button class="icon-btn small" aria-label={tr("Collapse side panel")} data-tip={inspectorHint(tr("Collapse side panel"))} data-tip-pos="left" onClick={close}><Icon name="panel-right-close" size={14} /></button></div>
    <div class="insp-scroll" classList={{ tasks: inspectorTab() === "tasks" }}><Switch><Match when={inspectorTab() === "plan"}><PlanAgents /></Match><Match when={inspectorTab() === "details" && selected()}><Details /></Match><Match when={inspectorTab() === "worktrees"}><Worktrees /></Match><Match when={inspectorTab() === "tasks"}><Suspense><TasksPanel /></Suspense></Match></Switch></div></Show>
  </aside></Show>;
}
