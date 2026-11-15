// Adapted from WhiteWh's TeamDrawers: the feed/inbox rows and drawer header.
// Our store returns one ascending snapshot page, not the fork's event cache.
import { For, Show, Switch, Match, Suspense, createEffect, createMemo, createSignal, lazy, on, onCleanup } from "solid-js";
import { tr } from "../../i18n";
import { teams } from "../../store/teams";
import { RecordedPageExport, exportRecordedPage, type PageExportPort } from "../../store/teamProfileExport";
import { Icon } from "../lib/icons";
import { TextInput } from "../lib/TextField";
import { TeamDrawerRead, readTeamDrawer, type DrawerKind, type DrawerRow, type TeamDrawer, type DrawerPort } from "../../store/teamDrawers";
import { feedClock, historyLine, inboxKind } from "./feed";
import { InboxText, Report, TeamReports, type DrawerAction } from "./TeamReports";
import { ProposalCards } from "./ProposalCards";
import type { TeamView } from "./types";
import "../styles/team-drawers.css";

const TeamTimeline = lazy(() => import("./TeamTimeline").then((module) => ({ default: module.TeamTimeline })));

const TeamLeadChat = lazy(() => import("./TeamLeadChat").then((module) => ({ default: module.TeamLeadChat })));

export const drawerLabel = (kind: TeamDrawer) => ({ timeline: tr("Timeline"), feed: tr("Feed"), inbox: tr("Inbox"), reports: tr("Reports"), cards: tr("Cards"), chat: tr("Lead chat") })[kind];

export function TeamDrawerButtons(props: { value: TeamDrawer | null; onOpen: (kind: TeamDrawer) => void }) {
  return <div class="team-drawer-buttons" role="group" aria-label={tr("Team drawers")}>
    <For each={["timeline", "feed", "inbox", "reports", "cards", "chat"] as const}>{(kind) =>
      <button type="button" class="btn small" aria-expanded={props.value === kind} data-team-drawer={kind} onClick={() => props.onOpen(kind)}>{drawerLabel(kind)}</button>
    }</For>
  </div>;
}

export function TeamFeed(props: { rows: DrawerRow[] }) {
  return <div class="team-drawer-rows" aria-label={tr("Feed")}>
    <For each={props.rows} fallback={<p class="dim">{tr("No events on this page.")}</p>}>{(row) =>
      <article class="team-drawer-row" data-feed-seq={row.seq}>
        <time class="mono" title={row.at_ms ? new Date(row.at_ms).toLocaleString() : ""}>{feedClock(row.at_ms)}</time>
        <span class="dim">{row.by}</span><span class="team-pill">{row.kind}</span>
        <pre class="team-drawer-text">{historyLine(row)}</pre>
        <Show when={row.text_truncated}><span class="dim">{tr("Only the beginning of this text is shown.")}</span></Show>
      </article>
    }</For>
  </div>;
}

function InboxRow(props: { row: DrawerRow; busy: boolean; onAction: DrawerAction }) {
  const [answer, setAnswer] = createSignal("");
  return <Show when={props.row.kind !== "report"} fallback={<Report row={props.row} busy={props.busy} onAction={props.onAction} />}>
    <article class="team-drawer-row" data-inbox={props.row.seq}>
      <header><span class="team-pill">{inboxKind(props.row.kind)}</span><b>{props.row.by}</b><span class="dim">{props.row.state}</span></header>
      <InboxText row={props.row} />
      <Show when={props.row.state === "open" && props.row.kind.startsWith("question:")}><div class="team-drawer-actions">
        <TextInput class="field" maxlength={4000} value={answer()} aria-label={tr("Your answer")} disabled={props.busy} onInput={(event) => setAnswer(event.currentTarget.value)} />
        <button type="button" class="btn small" disabled={props.busy || !answer().trim()} data-inbox-answer onClick={() => props.onAction("reply", { item: props.row.seq, text: answer() })}>{tr("Answer")}</button>
      </div></Show>
    </article>
  </Show>;
}

export function TeamInbox(props: { rows: DrawerRow[]; busy: boolean; onAction: DrawerAction }) {
  return <div class="team-drawer-rows" aria-label={tr("Inbox")}>
    <For each={props.rows.map((row) => row.seq)} fallback={<p class="dim">{tr("No inbox items on this page.")}</p>}>{(seq) => <Show when={props.rows.find((row) => row.seq === seq)}>{(row) => <InboxRow row={row()} busy={props.busy} onAction={props.onAction} />}</Show>}</For>
  </div>;
}

export function TeamDrawers(props: { kind: TeamDrawer; view: TeamView; busy: boolean; onAction: DrawerAction; onClose: () => void; port?: DrawerPort; visible?: boolean; exportPort?: PageExportPort }) {
  const [tick, setTick] = createSignal(0);
  const [readRevision, setReadRevision] = createSignal<number>();
  const model = new TeamDrawerRead(props.port ?? readTeamDrawer, () => setTick((n) => n + 1));
  const read = () => (tick(), model);
  const exporter = new RecordedPageExport(props.exportPort ?? exportRecordedPage, () => setTick((n) => n + 1));
  const exported = () => (tick(), exporter);
  const canExport = () => props.kind === "timeline" && !!read().page?.timeline && !read().loading && !read().error && !exported().busy;
  const active = () => props.kind !== "timeline" || (props.visible ?? teams.visible?.() ?? true);
  const target = createMemo(() => JSON.stringify([props.view.bar.id, props.kind, active()]));
  createEffect(on(target, () => {
    setReadRevision(props.view.revision);
    exporter.retire();
    if (!active() || props.kind === "chat") model.close();
    else model.open(props.view.bar.id, props.kind === "timeline" ? "feed" : props.kind === "reports" ? "inbox" : props.kind as DrawerKind);
  }));
  createEffect(on(() => props.view.revision, (revision) => { setReadRevision(revision); if (active() && props.kind !== "timeline") model.refresh(); }, { defer: true }));
  onCleanup(() => { model.close(); exporter.retire(); });
  const busy = () => props.busy || read().loading || !!read().error || readRevision() !== props.view.revision;
  const act: DrawerAction = (kind, fields) => { if (!busy()) props.onAction(kind, fields); };
  return <section class="team-drawer" aria-label={drawerLabel(props.kind)} data-team-drawer-panel={props.kind}>
    <header class="team-drawer-head"><b>{drawerLabel(props.kind)}</b><span class="grow" />
      <Show when={props.kind === "timeline"}>
        <button type="button" class="btn small" data-timeline-timings disabled={read().loading || !read().page || exported().busy} onClick={() => model.timings(!model.timingsRequested())}>{read().timingsRequested() ? tr("Events only") : tr("Read recorded timings")}</button>
        <button type="button" class="btn small" data-timeline-export disabled={!canExport()} onClick={() => { const page = model.page; if (canExport() && page) void exporter.save(page); }}>{tr("Export page…")}</button>
      </Show>
      <Show when={props.kind !== "chat"}><button type="button" class="btn small" disabled={read().loading} data-drawer-refresh onClick={() => model.refresh()}>{tr("Refresh")}</button></Show>
      <button type="button" class="icon-btn small" aria-label={tr("Close")} onClick={props.onClose}><Icon name="x" size={14} /></button>
    </header>
    <Show when={exported().outcome === "saved"}><p role="status">{tr("Page exported.")}</p></Show>
    <Show when={exported().outcome === "cancelled"}><p role="status">{tr("Export cancelled.")}</p></Show>
    <Show when={exported().outcome === "unknown"}><p role="alert">{tr("Export not confirmed: %1. Check the destination before exporting again.", exported().error)}</p></Show>
    <Show when={read().loading}><p role="status">{tr("Reading the team page…")}</p></Show>
    <Show when={read().error}>{(error) => <p class="error-text" role="alert">{error().startsWith("drawer_busy:") ? tr("A team drawer read is still finishing. Use Refresh.") : tr("Teams read failed: %1", error())}</p>}</Show>
    <Show when={props.kind === "chat"}><Suspense fallback={<p>{tr("Loading…")}</p>}><TeamLeadChat view={props.view} /></Suspense></Show>
    <Show when={read().page}>{(page) => <>
      <Switch>
        <Match when={props.kind === "timeline"}><Suspense fallback={<p>{tr("Loading…")}</p>}><TeamTimeline page={page()} /></Suspense></Match>
        <Match when={props.kind === "feed"}><TeamFeed rows={page().rows} /></Match>
        <Match when={props.kind === "inbox"}><TeamInbox rows={page().rows} busy={busy()} onAction={act} /></Match>
        <Match when={props.kind === "reports"}><TeamReports rows={page().rows} busy={busy()} onAction={act} /></Match>
        <Match when={props.kind === "cards"}><ProposalCards rows={page().rows} busy={busy()} onAction={act} /></Match>
      </Switch>
      <footer class="team-drawer-actions"><span class="dim">{tr("One page is shown. Refresh reads this page again.")}</span><span class="grow" />
        <Show when={page().after}><button type="button" class="btn small" disabled={read().loading} data-drawer-first onClick={() => model.first()}>{tr("First page")}</button></Show>
        <Show when={page().next}><button type="button" class="btn small" disabled={read().loading} data-drawer-next onClick={() => model.next()}>{tr("Next page")}</button></Show>
      </footer>
    </>}</Show>
  </section>;
}
