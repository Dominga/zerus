import { For, Show, createEffect, on, onCleanup, onMount } from "solid-js";
import { tasks } from "../../store/tasks";
import { copy, notify } from "../../store/actions";
import { copyReviewBusy, copyReviewCards, createCopyReviewPanel, dismissCopyReview, copyReviewOutcome, reviewingCopy, setReviewingCopy, confirmCopyReview } from "../../store/taskCopyReview";
import { tr } from "../../i18n";
import { Icon } from "../lib/icons";

export default function TaskCopyReview(props: { project: string; name: string }) {
  const panel = createCopyReviewPanel();
  const cards = () => copyReviewCards(props.project, tasks.keptDamaged());
  const more = () => Math.max(0, tasks.damaged() - cards().filter((row) => tasks.keptDamaged().some((live) => live.uuid === row.uuid)).length);
  let visibleProject = props.project;
  createEffect(on(() => props.project, (next) => {
    if (next !== visibleProject) panel.retire();
    visibleProject = next;
  }));
  onCleanup(panel.dispose);
  return <section class="tk-damaged-review" aria-label={tr("Damaged task copies")}>
    <For each={cards().map((row) => row.uuid)}>{(uuid) => {
      let origin: HTMLButtonElement | undefined;
      onCleanup(() => panel.retireOrigin(origin));
      const outcome = () => copyReviewOutcome(props.project, uuid);
      const title = () => tasks.find(uuid)?.title;
      return <article class="tk-card" aria-label={tr("Damaged task copy %1", uuid)}>
        <strong>{props.name}<Show when={title()}>: {title()}</Show></strong>
        <button type="button" class="irow copyable" title={tr("Click to copy")} aria-label={tr("Copy task UUID: %1", uuid)}
          onClick={() => void copy(uuid).then((result) => { if (result.outcome === "done") notify("info", tr("Copied")); })}>
          <span class="irow-val mono">{uuid}</span><span class="copy-mark" aria-hidden="true"><Icon name="copy" size={13} /></span>
        </button>
        <Show when={outcome()} fallback={<p class="dim">{cards().find((row) => row.uuid === uuid)?.error}</p>}>{(result) =>
          <p role="status">{result().state === "moved" ? tr("Local copy moved aside. The next received copy can be accepted.")
            : result().state === "refused" ? tr("Local copy review refused: %1", result().reason)
              : tr("Local copy outcome is unknown. Do not retry. Review the files manually. %1", result().reason)}</p>
        }</Show>
        <button type="button" class="btn small" ref={origin} disabled={copyReviewBusy() || !!reviewingCopy() || ["moved", "unknown"].includes(outcome()?.state ?? "")}
          onClick={(event) => void panel.open(props.project, uuid, event.currentTarget)}>{tr("Review local copy…")}</button>
        <Show when={outcome()?.state === "moved"}><button type="button" class="btn small" onClick={() => dismissCopyReview(props.project, uuid)}>{tr("Dismiss")}</button></Show>
      </article>;
    }}</For>
    <Show when={more()}><p class="dim">{tr("+%1 more damaged tasks", more())}</p></Show>
    <Show when={reviewingCopy()?.project === props.project && panel.owns(reviewingCopy()) && reviewingCopy()}>{(current) => <CopyReviewDialog current={current()} />}</Show>
  </section>;
}

function CopyReviewDialog(props: { current: NonNullable<ReturnType<typeof reviewingCopy>> }) {
  const current = props.current;
  let dialog!: HTMLDialogElement, cancel!: HTMLButtonElement;
  const close = () => { if (!copyReviewBusy() && reviewingCopy() === current) setReviewingCopy(undefined); };
  onMount(() => { dialog.showModal(); queueMicrotask(() => cancel.focus()); });
  onCleanup(() => { if (dialog.open) dialog.close(); queueMicrotask(() => { if (current.origin.isConnected) current.origin.focus(); }); });
  return <dialog ref={dialog} class="dlg" role="alertdialog" aria-labelledby="task-copy-review-title" aria-describedby="task-copy-review-body"
    onCancel={(event) => { event.preventDefault(); close(); }}>
    <div class="dlg-head"><h2 id="task-copy-review-title">{tr("Review local copy…")}</h2></div>
    <div class="dlg-body" id="task-copy-review-body"><p>{tr("Move aside the damaged local copy of task %1 and accept its next received copy? Attempt history and unsent local edits may be missing from that copy.", current.uuid)}</p></div>
    <div class="dlg-foot"><button type="button" class="btn" ref={cancel} autofocus disabled={copyReviewBusy()} onClick={close}>{tr("Cancel")}</button>
      <button type="button" class="btn solid-danger" disabled={copyReviewBusy()} onClick={() => void confirmCopyReview()}>{tr("Move copy aside")}</button></div>
  </dialog>;
}
