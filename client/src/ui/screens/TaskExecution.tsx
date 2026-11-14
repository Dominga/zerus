import { Show } from "solid-js";
import { tr, uiLanguage } from "../../i18n";
import type { ExecutionPhase, SavedExecution } from "./task-execution";

function phase(value: ExecutionPhase): string {
  switch (value) {
    case "submitted": return tr("Submitted — begin unconfirmed");
    case "running": return tr("Begin recorded");
    case "paused": return tr("Pause recorded");
    case "ready": return tr("Ready for review recorded");
    case "cancelled": return tr("Cancellation recorded");
    case "not_started": return tr("Not started");
    case "unknown": return tr("Delivery unknown");
    case "unconfirmed": return tr("Begin unconfirmed");
  }
}
function Stamp(props: { value: number | null }) {
  const date = () => props.value === null ? undefined : new Date(props.value);
  return <Show when={date()} fallback={tr("Not recorded")}>{(d) => <time datetime={d().toISOString()}>{d().toLocaleString(uiLanguage())}</time>}</Show>;
}

/** Pure presentation: no lookups, timers, task actions or native status inference. */
export function TaskExecution(props: { execution?: SavedExecution; stale?: boolean }) {
  return <section class="tv-execution" aria-label={tr("Saved execution")}>
    <header class="tv-section"><span class="eyebrow">{tr("Saved execution")}</span></header>
    <Show when={props.stale}><p class="notice">{tr("This saved execution could not be refreshed.")}</p></Show>
    <Show when={props.execution} fallback={<p>{tr("Execution details unavailable.")}</p>}>{(e) => <>
      <Show when={e().conflict}><p class="notice">{tr("Execution records conflict.")}</p></Show>
      <Show when={e().has_unknown}><p class="notice">{tr("Some attempt delivery is unknown.")}</p></Show>
      <Show when={e().has_unexplained}><p class="notice">{tr("Some execution history is unexplained.")}</p></Show>
      <Show when={e().current} fallback={<p>{tr("No current attempt recorded.")}</p>}>{(a) => <>
        <p class="tv-execution-phase">{phase(a().phase)}</p>
        <Show when={a().legacy}><p class="muted">{tr("Legacy execution record")}</p></Show>
        <dl>
          <Show when={!a().legacy}><dt>{tr("Attempt")}</dt><dd><code>{a().id}</code></dd><dt>{tr("Generation")}</dt><dd>{a().generation}</dd></Show>
          <dt>{tr("Requirement revision")}</dt><dd>{a().requirement}</dd>
          <dt>{tr("Current requirement revision")}</dt><dd>{e().requirement}</dd>
          <Show when={a().request_id}><dt>{tr("Request")}</dt><dd><code>{a().request_id}</code></dd></Show>
          <dt>{tr("Recorded executor")}</dt><dd><Show when={a().executor} fallback={tr("Not recorded")}>{(peer) => <>
            <span>{peer().session || tr("Not recorded")}</span><br /><code>{peer().session_id || tr("Not recorded")}</code><br />
            <span>{peer().machine}{peer().host ? ` @${peer().host}` : ""}</span>
            <Show when={!peer().resolved}><br /><span>{tr("Executor identity unresolved")}</span></Show>
          </>}</Show></dd>
          <dt>{tr("Recorded at")}</dt><dd><Stamp value={a().legacy && a().at === 0 ? null : a().at} /></dd>
          <dt>{tr("Begin recorded at")}</dt><dd><Stamp value={a().began_at} /></dd>
          <dt>{tr("Progress recorded at")}</dt><dd><Stamp value={a().progress_at} /></dd>
        </dl>
        </>}</Show>
    </>}</Show>
    <p class="muted">{tr("Saved evidence only. Live execution state and executor queue are unavailable.")}</p>
  </section>;
}
