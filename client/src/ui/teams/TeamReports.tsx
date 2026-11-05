import { For, Show, createMemo, createSignal } from "solid-js";
import { tr } from "../../i18n";
import { Icon } from "../lib/icons";
import { useModel, windowPort } from "./ProposalCards";
import { ReportDecision, reportLine, reportRows, type ReportRow } from "./decisions";
import type { ComputerId, TeamView } from "../../types";

// The reports the owner decides in the Team tab (task 11, Qt TeamPanel):
// an escalated report is accepted or rejected with the member's comment; an
// attempt without a report takes the member's last reply. Each sends once;
// a refusal says why on its row.

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
          <input class="field" maxlength={4000} placeholder={tr("What should the member change?")} value={comment()} disabled={read().sending}
            onInput={(event) => setComment(event.currentTarget.value)} />
          <button class="btn small danger" disabled={read().sending || !comment().trim()} onClick={() => void model.reject(props.row, comment())}>{tr("Reject")}</button>
        </div>
      </Show>
      <Show when={read().notice}><div class="team-proposal-notice" role="status">{read().notice}</div></Show>
    </div>
  );
}

export function TeamReports(props: { computer: ComputerId; team: TeamView }) {
  const rows = createMemo(() => reportRows(props.team));
  return (
    <Show when={rows().length > 0}>
      <div class="team-reports" role="list" aria-label={tr("Reports")}>
        <For each={rows().map((row) => `${row.kind}:${row.task}`)}>{(key) => (
          <Show when={rows().find((row) => `${row.kind}:${row.task}` === key)}>{(row) => <Report computer={props.computer} team={props.team} row={row()} />}</Show>
        )}</For>
      </div>
    </Show>
  );
}
