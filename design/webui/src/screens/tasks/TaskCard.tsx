import { Show } from "solid-js";
import { Icon } from "../../lib/icons";
import { StatusBadge } from "../sessions/SessionCard";
import {
  ACTION_ICON, ACTION_LABEL, STAGE_LABEL, act, actionsOf, ageText, belongsTo, blockedOf, displayTitle, executorSession,
  openTask, primaryOf, queueOf, targetText, workerLabel, type TaskVM,
} from "./board";
import { below, openTaskMenu, showReason, type TaskMenuItem } from "./overlays";

/** The stage circle: To do shows its queue number, review and done a check, In progress a spinning half ring. */
export function TaskGlyph(props: { task: TaskVM; size?: "lg" }) {
  return (
    <span class={`tk-glyph st-${props.task.stage}`} classList={{ lg: props.size === "lg" }} aria-hidden="true">
      <Show when={props.task.stage === "todo"}>{queueOf(props.task)}</Show>
      <Show when={props.task.stage === "review" || props.task.stage === "done"}><Icon name="check" size={props.size === "lg" ? 12 : 11} stroke={3} /></Show>
    </span>
  );
}

/** The same actions in the same order on every surface; blocked ones explain themselves. */
export function taskMenuItems(t: TaskVM): TaskMenuItem[] {
  const blocked = blockedOf(t);
  return actionsOf(t.stage).map((a) => ({ label: ACTION_LABEL[a], icon: ACTION_ICON[a], reason: blocked[a], danger: a === "cancel", action: () => act(t, a) }));
}

/**
 * One task card in the side panel (TaskRow, compact): stage glyph, the title
 * clamped to two lines, a meta line (ID, target or worker, In progress status,
 * age, accepted date, filer), the review summary, and on the right the primary
 * action plus the action menu. Blocked actions stay clickable, dimmed, and say why.
 */
export function TaskCard(props: { task: TaskVM; selected: boolean; showStage: boolean; dragging: boolean; onDragStart: (e: DragEvent) => void; onDragEnd: () => void }) {
  const t = () => props.task;
  const primary = () => primaryOf(t().stage);
  const why = () => blockedOf(t())[primary()];
  const status = () => (t().stage === "active" && !t().worker ? executorSession(t()) : undefined);
  const quiet = () => t().stage === "backlog" || t().stage === "done";
  const menu = (e: MouseEvent | KeyboardEvent, anchor?: Element) => {
    e.preventDefault(); e.stopPropagation();
    const at = anchor ? below(anchor) : e instanceof MouseEvent ? { x: e.clientX, y: e.clientY } : below(e.currentTarget as Element);
    openTaskMenu(at, "Task actions", taskMenuItems(t()));
  };
  const runPrimary = (e: MouseEvent) => {
    e.stopPropagation();
    const reason = why();
    if (reason) { openTask(t().id); showReason(e.currentTarget as Element, reason); return; }
    act(t(), primary());
  };
  return (
    <article class={`tk-card st-${t().stage}`} data-task={t().id} data-stage={t().stage} tabindex="0" role="listitem"
      aria-label={`${t().id}: ${displayTitle(t())}`} aria-current={props.selected ? "true" : undefined}
      classList={{ sel: props.selected, mine: belongsTo(t()), quiet: quiet(), dragging: props.dragging, readonly: !t().writable }}
      draggable={t().writable} onDragStart={(e) => props.onDragStart(e)} onDragEnd={() => props.onDragEnd()}
      onClick={() => openTask(t().id)} onContextMenu={(e) => menu(e)}
      onKeyDown={(e) => {
        if (e.target !== e.currentTarget) return;
        if (e.key === "Enter" || e.key === " ") { e.preventDefault(); openTask(t().id); }
        else if (e.key === "ContextMenu" || (e.shiftKey && e.key === "F10")) menu(e);
      }}>
      <TaskGlyph task={t()} />
      <div class="tk-card-main">
        <div class="tk-card-title">{displayTitle(t())}</div>
        <div class="tk-meta">
          <Show when={props.showStage}><span class="tk-meta-stage">{STAGE_LABEL[t().stage]}</span></Show>
          <span class="mono tk-id">{t().id}</span>
          <Show when={targetText(t())}><span class="tk-target ell" classList={{ mine: belongsTo(t()) }} title={targetText(t())}>{targetText(t())}</span></Show>
          <Show when={status()}>{(s) => <StatusBadge kind={s().kind} caption={s().status} />}</Show>
          <Show when={t().stage === "active"}><span class="tk-age">{ageText(t().stageAgeMin)}</span></Show>
          <Show when={t().stage === "done" && t().acceptedOn}><span class="tk-age">accepted {t().acceptedOn}</span></Show>
          <Show when={t().author}>{(a) => <span class="tk-filed ell">filed by {a().host ? `${a().name} @${a().host}` : a().name}</span>}</Show>
        </div>
        <Show when={t().stage === "review" && (t().cancelledAt || t().summary)}>
          <p class="tk-summary">
            <Show when={t().cancelledAt} fallback={<><b>{workerLabel(t())} reported:</b> {t().summary}</>}>
              <b>Cancelled at {t().cancelledAt}{t().cancelReason ? ":" : ""}</b> {t().cancelReason}
            </Show>
          </p>
        </Show>
      </div>
      <div class="tk-card-acts">
        <button class="icon-btn tk-act tk-primary" classList={{ blocked: !!why() }} aria-label={ACTION_LABEL[primary()]}
          aria-description={why()} title={why() ?? ACTION_LABEL[primary()]} onClick={runPrimary} draggable={false}>
          <Icon name={ACTION_ICON[primary()]} size={15} />
        </button>
        <button class="icon-btn tk-act" aria-label="Task actions" title="Task actions" aria-haspopup="menu" draggable={false}
          onClick={(e) => menu(e, e.currentTarget)}>
          <Icon name="ellipsis" size={15} />
        </button>
      </div>
    </article>
  );
}

