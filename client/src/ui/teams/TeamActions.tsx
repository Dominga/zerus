import { For, Show, onCleanup } from "solid-js";
import { tr } from "../../i18n";
import { menu, setMenu, type MenuEntry } from "../../store/ui";
import { Icon } from "../lib/icons";
import { memberActions, taskButtons, type TaskButton } from "./actions";
import { barActions } from "./actions";
import type { MemberView, TaskView, TeamView } from "./types";

// The Team tab's action buttons. They only name the operation and call the view's
// one action callback; the store sends it and the core decides the outcome.

export function TeamBarActions(props: { view: TeamView; busy: boolean; onAction: (kind: string) => void }) {
  return (
    <span class="team-bar-actions" data-strip-actions>
      <Show when={["running", "paused"].includes(props.view.bar.status) && props.view.bar.integration_branch}>
        <button type="button" class="btn small" disabled={props.busy} data-advance-base data-strip-action="advance_base" onClick={() => props.onAction("advance_base")}>{tr("Advance base…")}</button>
      </Show>
      <For each={barActions(props.view)}>
        {(action) => (
          <button type="button" class="btn small" classList={{ danger: action.danger }} disabled={props.busy || action.disabled}
            data-strip-action={action.id} onClick={() => props.onAction(action.id)}>{action.text}</button>
        )}
      </For>
    </span>
  );
}

/** The decisions a card offers; a reason button asks for its text first. */
export function TaskActions(props: {
  card: TaskView;
  /** The team's status: the store takes task actions only from an active team. */
  teamStatus: string;
  busy: boolean;
  onAction: (kind: string) => void;
  onReason: (button: TaskButton) => void;
}) {
  // A task with no action has no row: neither its divider nor an empty line.
  const buttons = () => taskButtons(props.card, props.teamStatus);
  /** Why a shown decision is disabled, each reason once. */
  const why = () => [...new Set(buttons().flatMap((button) => button.blocked ? [button.blocked] : []))];
  /** What an open decision does now, each line once. */
  const notes = () => [...new Set(buttons().flatMap((button) => button.note ? [button.note] : []))];
  return (
    <Show when={buttons().length}>
      <span class="team-finding-actions" data-task-actions>
        <For each={buttons()}>
          {(button) => (
            <button type="button" class="btn small" classList={{ danger: button.danger, primary: button.id === "accept" }}
              disabled={props.busy || !!button.blocked} title={button.blocked} data-task-action={button.id}
              onClick={() => (button.reason ? props.onReason(button) : props.onAction(button.id))}>{button.text}</button>
          )}
        </For>
        <For each={why()}>{(reason) => <span class="team-action-why dim" data-action-why>{reason}</span>}</For>
        <For each={notes()}>{(line) => <span class="team-action-why dim" data-action-note>{line}</span>}</For>
      </span>
    </Show>
  );
}

/** A member's operations in its card. */
export function MemberActions(props: { member: MemberView; teamStatus: string; busy: boolean; onAction: (kind: string, member: string) => void; onDetails?: () => void; detailsOpen?: boolean }) {
  let items: MenuEntry[] | undefined;
  onCleanup(() => { if (items && menu()?.items === items) setMenu(undefined); });
  return (
    <Show when={memberActions(props.member, props.teamStatus).length || props.onDetails}>
      <span class="team-member-actions" data-member-actions>
        <button type="button" class="icon-btn small team-member-more" aria-label={tr("Member actions")} title={tr("Member actions")} aria-haspopup="menu" disabled={props.busy}
          onClick={(event) => {
            const trigger = event.currentTarget, box = trigger.getBoundingClientRect(), memberId = props.member.id;
            items = [
              // An item like its neighbours, with its icon; it says Hide while the details are open.
              ...(props.onDetails ? [{ label: props.detailsOpen ? tr("Hide member details") : tr("Member details"), icon: "info", action: () => { trigger.focus(); props.onDetails?.(); } }] : []),
              ...memberActions(props.member, props.teamStatus).map((action) => ({
              label: action.text, icon: action.id.startsWith("pause") ? "pause" : action.id.startsWith("resume") ? "play" : action.id.startsWith("stop") ? "square" : "trash",
              danger: action.danger, disabled: props.busy || action.disabled, hint: action.reason,
              action: () => { if (props.busy || action.disabled) return; trigger.focus(); props.onAction(action.id, memberId); },
            }))];
            setMenu({ x: box.right - 200, y: box.bottom + 4, items });
          }}><Icon name="ellipsis" size={14} /></button>
      </span>
    </Show>
  );
}
