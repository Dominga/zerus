import { For, Show, createMemo, createSignal } from "solid-js";
import { teams } from "../../store/teams";
import { teamsOlder } from "../../bridge";
import { Icon } from "../lib/icons";
import { tr } from "../../i18n";
import { feedClock, focusedLines, historyLine, inboxKind } from "./history";
import { roleStyle } from "./roles";
import type { ComputerId, TeamFeedEvent, TeamView } from "../../types";

// Task 198 (W1): the lead's Inbox and the team's Feed under the plan, as Qt's
// TeamWorkspace draws them (inboxStrip, feed drawer): each folds to one line
// that names its count and newest item; open, the Inbox lists every item
// oldest first with the command that answers it, the Feed every event newest
// first with Load older. The core reads both while the team is watched
// (`inbox:<id>`, `feed:<id>`); Load older asks it for the page before.

const [inboxOpen, setInboxOpen] = createSignal(false);
const [feedOpen, setFeedOpen] = createSignal(false);

export function TeamInbox(props: { computer: ComputerId; team: TeamView }) {
  const items = () => teams.inbox(props.computer, props.team.id)?.items ?? [];
  const where = (task: number | null) => task ? ` #${task}` : "";
  return (
    <Show when={items().length}>
      <section class="team-drawer team-inbox" aria-label={tr("Inbox")} classList={{ open: inboxOpen() }}>
        <header class="team-drawer-head">
          <button class="icon-btn small" aria-expanded={inboxOpen()} aria-label={tr("Inbox")} data-inbox-toggle onClick={() => setInboxOpen(!inboxOpen())}>
            <Icon name={inboxOpen() ? "chevron-down" : "chevron-right"} size={13} />
          </button>
          <b>{tr("Inbox")}</b>
          <span class="accent">{items().length === 1 ? tr("1 for the lead") : tr("%1 for the lead", items().length)}</span>
          <Show when={!inboxOpen()} fallback={<span class="grow" />}>
            <span class="team-drawer-latest ell" title={items()[0].command}>{`${inboxKind(items()[0].kind)} ${items()[0].member}${where(items()[0].task)} ${items()[0].summary}`}</span>
          </Show>
        </header>
        <Show when={inboxOpen()}>
          <div class="team-drawer-rows" style={{ "max-height": `${Math.min(170, items().length * 24 + 6)}px` }}>
            <For each={items()}>{(item) => (
              <div class="team-drawer-row" data-inbox={item.kind}>
                <span class="team-pill" classList={{ warn: item.kind === "report" }}>{inboxKind(item.kind)}</span>
                <span class="dim">{item.member}</span>
                <Show when={item.task}><span class="muted">#{item.task}</span></Show>
                <span class="ell grow" title={item.command}>{item.summary}</span>
              </div>
            )}</For>
          </div>
        </Show>
      </section>
    </Show>
  );
}

export function TeamFeed(props: { computer: ComputerId; team: TeamView; focusRole?: string }) {
  const feed = () => teams.feed(props.computer, props.team.id);
  const [asking, setAsking] = createSignal(false);
  const lines = createMemo(() => (feed()?.events ?? []).map((event: TeamFeedEvent) => ({ ...historyLine(event, props.team.tasks), at: event.at, seq: event.seq })));
  const shown = createMemo(() => focusedLines(lines(), props.focusRole ?? ""));
  const older = async () => {
    if (asking()) return;
    setAsking(true);
    try { await teamsOlder(props.computer); } finally { setAsking(false); }
  };
  return (
    <Show when={feed()}>
      <section class="team-drawer team-feed" aria-label={tr("Feed")} classList={{ open: feedOpen() }}>
        <header class="team-drawer-head">
          <button class="icon-btn small" aria-expanded={feedOpen()} aria-label={tr("Feed")} data-feed-toggle onClick={() => setFeedOpen(!feedOpen())}>
            <Icon name={feedOpen() ? "chevron-down" : "chevron-right"} size={13} />
          </button>
          <b>{tr("Feed")}</b>
          <span class="muted">{lines().length === 1 ? tr("1 event") : tr("%1 events", lines().length)}</span>
          <Show when={!feedOpen() && lines().length} fallback={<span class="grow" />}>
            <span class="team-drawer-latest ell">{`${feedClock(lines()[0].at)} ${lines()[0].text}`}</span>
          </Show>
          <Show when={feedOpen() && feed()!.more}>
            <button class="btn small ghost" data-feed-older disabled={asking()} onClick={() => void older()}>{tr("Load older")}</button>
          </Show>
        </header>
        <Show when={feedOpen()}>
          <div class="team-drawer-rows feed" style={{ height: "170px" }}>
            <For each={shown()}>{(line) => (
              <div class="team-drawer-row" data-seq={line.seq}>
                <span class="team-feed-time mono">{feedClock(line.at)}</span>
                <Show when={line.by}><span class="role-badge" style={roleStyle(line.by, line.by === "Lead")}>{line.by}</span></Show>
                <span class={`team-feed-text tone-${line.tone}`}>{line.text}</span>
              </div>
            )}</For>
            <Show when={feed()!.capped}><div class="dim team-feed-capped">{tr("Older events are not kept here.")}</div></Show>
          </div>
        </Show>
      </section>
    </Show>
  );
}
