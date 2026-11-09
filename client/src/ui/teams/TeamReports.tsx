import { For, Show, createEffect, createMemo, createSignal, onCleanup } from "solid-js";
import { tr } from "../../i18n";
import { Icon } from "../lib/icons";
import { useModel, windowPort } from "./ProposalCards";
import { ReportDecision, TaskReportRead, reportLine, reportRows, type ReportRow } from "./decisions";
import { teamTaskRead } from "../../bridge";
import type { ComputerId, TeamView } from "../../types";
import { TextInput } from "../lib/TextField";

// The reports the lead or the owner decides in the Team tab (task 11, Qt
// TeamPanel): an escalated report is accepted or rejected with the member's
// comment; an attempt without a report takes the member's last reply (task #362:
// the lead and the member's sub-lead may take it too, so the block stays in the
// lead's session; a member cannot use its own). Each sends once; a refusal says
// why on its row.

function Report(props: { computer: ComputerId; team: TeamView; row: ReportRow }) {
  const { model, tick } = useModel((changed) => new ReportDecision(windowPort, { computer: props.computer, team: props.team.id, teamName: props.team.name }, changed));
  const read = () => (tick(), model);
  const [comment, setComment] = createSignal("");
  const [rejecting, setRejecting] = createSignal(false);
  return (
    <div class="team-report" data-task={props.row.task} data-kind={props.row.kind}>
      <div class="team-report-line">
        <Icon name={props.row.kind === "report" ? "message-square" : "hourglass"} size={13} />
        <span class="ell" title={reportLine(props.row)}>{reportLine(props.row)}</span>
        <span class="grow" />
        <Show when={props.row.kind === "report"} fallback={
          <button class="btn small primary" disabled={read().sending} onClick={() => void model.useLastReply(props.row)}>{tr("Use last reply as report")}</button>
        }>
          <button class="btn small" disabled={read().sending} onClick={() => setRejecting(!rejecting())}>{tr("Reject…")}</button>
          <button class="btn small primary" disabled={read().sending} onClick={() => void model.accept(props.row)}>{tr("Accept")}</button>
        </Show>
      </div>
      <Show when={rejecting() && props.row.kind === "report"}>
        <div class="team-proposal-answers">
          <TextInput class="field" maxlength={4000} placeholder={tr("What should the member change?")} value={comment()} disabled={read().sending}
            onInput={(event) => setComment(event.currentTarget.value)} />
          <button class="btn small danger" disabled={read().sending || !comment().trim()} onClick={() => void model.reject(props.row, comment())}>{tr("Reject")}</button>
        </div>
      </Show>
      <Show when={read().notice}><div class="team-proposal-notice" role="status">{read().notice}</div></Show>
    </div>
  );
}

export function ArchivedTaskReport(props: { computer: ComputerId; team: string; task: number; onClose: () => void }) {
  const { model, tick } = useModel((changed) => new TaskReportRead(teamTaskRead, changed));
  const read = () => (tick(), model);
  const load = () => void model.open(props.computer, props.team, props.task);
  createEffect(load);
  onCleanup(() => model.close());
  return (
    <section class="team-report" aria-label={tr("Archived report")}>
      <div class="team-report-line">
        <b>{tr("Archived task #%1", props.task)}</b>
        <span class="grow" />
        <button class="btn small" onClick={props.onClose}>{tr("Close")}</button>
      </div>
      <Show when={read().loading}><p role="status">{tr("Reading the archived report…")}</p></Show>
      <Show when={read().error}>
        <p class="error-text" role="alert">{read().error}</p>
        <button class="btn small" onClick={load}>{tr("Retry")}</button>
      </Show>
      <Show when={read().detail}>{(detail) => (
        <>
          <h3>{detail().task.title}</h3>
          <pre style={{ "white-space": "pre-wrap", "overflow-wrap": "anywhere" }}>{detail().report || tr("No report was recorded for this task")}</pre>
          <Show when={detail().report_truncated}><p>{tr("Only the beginning of this report is shown")}</p></Show>
        </>
      )}</Show>
    </section>
  );
}

export function TeamReports(props: { computer: ComputerId; team: TeamView; onOpenReport?: (id: number) => void }) {
  const rows = createMemo(() => reportRows(props.team));
  const archived = createMemo(() => props.team.tasks.filter((task) => task.archived));
  const [page, setPage] = createSignal(0);
  const start = () => Math.min(page() * 50, Math.max(0, Math.ceil(archived().length / 50) - 1) * 50);
  return (
    <>
    <Show when={rows().length > 0}>
      <div class="team-reports" role="list" aria-label={tr("Reports")}>
        <For each={rows().map((row) => `${row.kind}:${row.task}`)}>{(key) => (
          <Show when={rows().find((row) => `${row.kind}:${row.task}` === key)}>{(row) => <Report computer={props.computer} team={props.team} row={row()} />}</Show>
        )}</For>
      </div>
    </Show>
    <Show when={archived().length > 0}>
      <details class="team-reports">
        <summary>{tr("Archived reports (%1)", archived().length)}</summary>
        <For each={archived().slice(start(), start() + 50)}>{(task) => (
          <button class="btn small" onClick={() => props.onOpenReport?.(task.id)}>{`#${task.id} ${task.title}`}</button>
        )}</For>
        <Show when={archived().length > 50}>
          <button class="btn small" disabled={start() === 0} onClick={() => setPage(Math.max(0, page() - 1))}>{tr("Previous")}</button>
          <button class="btn small" disabled={start() + 50 >= archived().length} onClick={() => setPage(page() + 1)}>{tr("Next")}</button>
        </Show>
      </details>
    </Show>
    </>
  );
}
