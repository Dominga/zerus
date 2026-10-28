import { For, Show } from "solid-js";
import { Icon } from "../lib/icons";
import { ActivityView, columnStyle } from "./Activity";
import { AgentBadge, MachineBadge, statusOf, folderOf } from "./SessionCard";
import { selected, setMenu, subagentOpen, toast, type MenuEntry } from "../../store/ui";
import { activity } from "../../store/session";
import { detailsFor } from "../../store/actions";
import { actionEntries, entryOf } from "./ActionDialogs";

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

const copy = (text: string, what: string) => {
  void navigator.clipboard?.writeText(text).then(() => toast("Copied", what), () => toast("Not copied", "The clipboard is unavailable"));
};

function Header() {
  const s = () => selected()!;
  const tone = () => (statusOf(s()) === "attention" ? "attention" : statusOf(s()));
  const act = () => {
    const entry = entryOf(s());
    return actionEntries(s(), entry ? detailsFor(entry) : undefined);
  };
  const actions = (): MenuEntry[] => [
    { label: "Open folder", icon: "folder-open", disabled: true },
    { label: "Open shell in session folder", icon: "square-terminal", disabled: true },
    { label: "Copy session name", icon: "copy", action: () => copy(s().name, "The session name is on the clipboard") },
    { separator: true },
    act()?.rename ?? { label: "Rename…", icon: "pencil", disabled: true },
    { label: "Fork session…", icon: "git-fork", disabled: true },
    { separator: true },
    { label: "Copy connect command", icon: "link", disabled: true },
    act()?.change ?? { label: "Pause session", icon: "pause", disabled: true },
    ...(act()?.stop && !act()!.stop.disabled ? [act()!.stop] : []),
    ...(act()?.markRead ? [act()!.markRead] : []),
    { separator: true },
    { label: "Clear session…", icon: "clear", disabled: true },
    ...(act() ? (act()!.archive ? [act()!.archive!] : []) : [{ label: "Move to archive", icon: "archive", disabled: true }]),
    act()?.end ?? { label: "Terminate session…", icon: "circle-stop", danger: true, disabled: true },
  ];
  return (
    <header class="detail-head">
      <div class="dh-row1">
        <h1 class="ell" title={s().name}>{s().project} / {s().title}<Show when={subagentOpen()}> / {s().subagents?.find((k) => k.id === subagentOpen())?.name}</Show></h1>
        <Show when={s().computer !== "" && statusOf(s()) === "working" && !activity.loaded()}><span class="busy-arc" title="Waiting for the remote read" /></Show>
        <span class="grow" />
        <span class={`hbadge t-${tone()}`}>{headerCaption()}</span>
        <Show when={act()}>
          {(a) => (
            <>
              <Show when={!a().stop.disabled}><button class="icon-btn" data-tip="Stop the current turn" onClick={() => a().stop.action()}><Icon name="circle-stop" size={16} /></button></Show>
              <button class="icon-btn" data-tip={a().change.hint ?? a().change.label} disabled={a().change.disabled} aria-label={a().change.label}
                onClick={() => a().change.action()}><Icon name={a().change.icon!} size={16} /></button>
            </>
          )}
        </Show>
        <button class="icon-btn" data-tip="Open session folder in file manager" disabled><Icon name="folder-open" size={16} /></button>
        <button class="icon-btn" data-tip="Open shell in session folder" disabled><Icon name="square-terminal" size={16} /></button>
        <button class="icon-btn" data-tip="Copy session name" onClick={() => copy(s().name, "The session name is on the clipboard")}><Icon name="copy" size={15} /></button>
        <button class="icon-btn" data-tip="Clear session" disabled><Icon name="clear" size={15} /></button>
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
