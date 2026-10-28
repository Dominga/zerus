import { For, Show, createMemo } from "solid-js";
import { Icon } from "../lib/icons";
import { ActivityView, columnStyle } from "./Activity";
import { AgentBadge, MachineBadge, statusOf, folderOf } from "./SessionCard";
import { selected, setMenu, subagentOpen, type MenuEntry } from "../../store/ui";
import { activity } from "../../store/session";
import { computerOffline } from "../../store/catalog";
import { copyName, detailsFor, followedFor } from "../../store/actions";
import { headerClearSlot } from "./actions-ui";
import { actionEntries, freshActionEntry } from "./ActionDialogs";
import { snapshotOf } from "./actions-ui";
import { markEntry } from "./mark-entries";

// The session header and its Activity, after design/webui SessionDetail. The
// composer, Terminal, Processes and the side panel move in with later phases;
// their actions show disabled until then.

function headerCaption(): string {
  const s = selected();
  if (!s) return "";
  return statusOf(s) === "offline" ? "Offline" : s.caption;
}

function EmptyDetail() {
  return (
    <div class="detail-empty">
      <h2>A little more focus.</h2>
      <p>Select a session to see what your agent is doing.<br />New sessions appear here automatically.</p>
    </div>
  );
}

function Header() {
  const s = () => selected()!;
  const tone = () => (statusOf(s()) === "attention" ? "attention" : statusOf(s()));
  // Details come from the followed session's Activity state value; the store
  // inspects only once that subscription answered without one.
  const details = createMemo(() => detailsFor(s(), !computerOffline(s().computer), followedFor(s().key)));
  const act = createMemo(() => actionEntries(s(), details()));
  const mark = createMemo(() => markEntry(s()));
  const slot = () => headerClearSlot(!subagentOpen(), s().state);
  const fresh = createMemo(() => freshActionEntry(s(), true));
  const actions = (): MenuEntry[] => [
    { label: "Open folder", icon: "folder-open", disabled: true },
    { label: "Open shell in session folder", icon: "square-terminal", disabled: true },
    { label: "Copy session name", icon: "copy", action: () => { void copyName(snapshotOf(s())); } },
    { separator: true },
    act().rename,
    { label: "Fork session…", icon: "git-fork", disabled: true },
    { separator: true },
    { label: "Copy connect command", icon: "link", disabled: true },
    act().change,
    ...(act().stop.disabled ? [] : [act().stop]),
    ...(mark() ? [mark()!] : []),
    { separator: true },
    ...(slot() === "fresh" && fresh() ? [fresh()!] : slot() === "clear" ? [{ label: "Clear session…", icon: "clear", disabled: true }] : []),
    ...(act().archive ? [act().archive!] : []),
    act().end,
  ];
  return (
    <header class="detail-head">
      <div class="dh-row1">
        <h1 class="ell" title={s().name}>{s().project} / {s().title}<Show when={subagentOpen()}> / {s().subagents?.find((k) => k.id === subagentOpen())?.name}</Show></h1>
        <Show when={s().computer !== "" && statusOf(s()) === "working" && !activity.loaded()}><span class="busy-arc" title="Waiting for the remote read" /></Show>
        <span class="grow" />
        <span class={`hbadge t-${tone()}`}>{headerCaption()}</span>
        {/* Qt's composer Stop: shown while the turn works, enabled when the host can interrupt it. */}
        <Show when={act().stop.shown}>
          <button class="icon-btn" data-tip={act().stop.hint ?? "Stop the current turn"} aria-label="Stop the current turn" disabled={act().stop.disabled}
            onClick={() => act().stop.action()}><Icon name="circle-stop" size={16} /></button>
        </Show>
        <button class="icon-btn" data-tip={act().change.hint ?? act().change.label} disabled={act().change.disabled} aria-label={act().change.label}
          onClick={() => act().change.action()}><Icon name={act().change.icon!} size={16} /></button>
        <button class="icon-btn" data-tip="Open session folder in file manager" disabled><Icon name="folder-open" size={16} /></button>
        <button class="icon-btn" data-tip="Open shell in session folder" disabled><Icon name="square-terminal" size={16} /></button>
        <button class="icon-btn" data-tip="Copy session name" onClick={() => { void copyName(snapshotOf(s())); }}><Icon name="copy" size={15} /></button>
        <Show when={slot() === "clear"}><button class="icon-btn" data-tip="Clear session" disabled><Icon name="clear" size={15} /></button></Show>
        <button class="icon-btn" data-tip="Session actions" onClick={(e) => setMenu({ x: e.clientX - 200, y: e.clientY + 12, items: actions() })}><Icon name="ellipsis" size={16} /></button>
      </div>
      <div class="dh-row2">
        <AgentBadge agent={s().agent} />
        <MachineBadge alias={s().machine} computer={s().computer} />
        {/* The branch is a label beside the agent and machine badges, with the branch glyph. */}
        <Show when={s().branch}>
          <span class="dh-branch" title={`Git branch ${s().branch}`}><Icon name="git-branch" size={12} /><span class="ell">{s().branch}</span></span>
        </Show>
        <Show when={s().cwd}>
          <span class="dh-chip ell" title={s().cwd}><Icon name="folder" size={13} />{folderOf(s())}</span>
        </Show>
        <span class="grow" />
        <span class="dh-model ell">{s().model} {s().effort ?? ""}</span>
      </div>
    </header>
  );
}

const TABS = [{ id: "activity", label: "Activity" }] as const;

export function SessionDetail() {
  return (
    <section class="detail" aria-label="Session">
      <Show when={selected()} fallback={<EmptyDetail />}>
        <Header />
        <div class="work">
          <div class="work-main">
            <div class="detail-tabs tabs" role="tablist">
              <For each={TABS}>{(t) => (
                <button class="tab on" role="tab" aria-selected="true">{t.label}</button>
              )}</For>
              <span class="grow" />
            </div>
            <div class="tab-body">
              <div class="activity-wrap" style={columnStyle()}>
                <ActivityView />
              </div>
            </div>
          </div>
        </div>
      </Show>
    </section>
  );
}
