// Adapted from WhiteWh's report rows and reason editor, using our report inbox
// and existing Accept/Return operations. No last-reply or archive authority.
import { For, Show, createSignal } from "solid-js";
import { tr } from "../../i18n";
import { TextInput } from "../lib/TextField";
import type { DrawerRow } from "../../store/teamDrawers";
import { waitingNote } from "./decisions";
import { reportRows } from "./feed";

export type DrawerAction = (kind: string, fields: Record<string, unknown>) => void;

export function InboxText(props: { row: DrawerRow }) {
  return <>
    <pre class="team-drawer-text">{props.row.text}</pre>
    <Show when={props.row.text_truncated}><p class="dim">{tr("Only the beginning of this text is shown.")}</p></Show>
    <Show when={props.row.notes.length}><p class="dim">{tr("Service notes")}</p>
      <For each={props.row.notes}>{(note) => <p class="dim">{waitingNote(note)}</p>}</For>
    </Show>
    <Show when={props.row.reply}>{(reply) => <p class="team-drawer-reply">{tr("Reply: %1", reply())}</p>}</Show>
  </>;
}

export function Report(props: { row: DrawerRow; busy: boolean; onAction: DrawerAction }) {
  const [rejecting, setRejecting] = createSignal(false);
  const [reason, setReason] = createSignal("");
  const actionable = () => props.row.state === "open" && props.row.node !== null;
  return <article class="team-report" data-report={props.row.seq}>
    <header><b>{tr("Task %1", props.row.node ?? "")}</b><span class="dim">{props.row.by}</span><span class="dim">{props.row.state}</span></header>
    <InboxText row={props.row} />
    <Show when={actionable()}>
      <div class="team-drawer-actions">
        <button type="button" class="btn small" disabled={props.busy} onClick={() => setRejecting(!rejecting())}>{tr("Reject…")}</button>
        <button type="button" class="btn small primary" disabled={props.busy} data-report-accept onClick={() => props.onAction("accept", { node: props.row.node })}>{tr("Accept")}</button>
      </div>
      <Show when={rejecting()}>
        <div class="team-drawer-actions">
          <TextInput class="field" maxlength={4000} value={reason()} aria-label={tr("Reason")} placeholder={tr("What should the member change?")} disabled={props.busy} onInput={(event) => setReason(event.currentTarget.value)} />
          <button type="button" class="btn small danger" disabled={props.busy || !reason().trim()} data-report-return onClick={() => props.onAction("return", { node: props.row.node, reason: reason() })}>{tr("Reject")}</button>
        </div>
      </Show>
    </Show>
  </article>;
}

export function TeamReports(props: { rows: DrawerRow[]; busy: boolean; onAction: DrawerAction }) {
  return <div class="team-reports" role="list" aria-label={tr("Reports")}>
    <For each={reportRows(props.rows).map((row) => row.seq)} fallback={<p class="dim">{tr("No reports on this page.")}</p>}>
      {(seq) => <Show when={props.rows.find((row) => row.seq === seq)}>{(row) => <Report row={row()} busy={props.busy} onAction={props.onAction} />}</Show>}
    </For>
  </div>;
}
