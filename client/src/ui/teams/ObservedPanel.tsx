import { For, Show, createEffect, createSignal, onCleanup } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import "../styles/observed-timing.css";
import { tr } from "../../i18n";
import { observedFollower, type ObservedFollow, type ObservedPane } from "../../store/observed-timing";
const time = (value: number | null) => value === null ? tr("Unavailable") : new Date(value).toLocaleString();
const category = (value: string) => {
  switch (value) {
    case "delivery_reserved": return tr("Delivery reserved"); case "delivery_submitted": return tr("Delivery submitted");
    case "delivery_refused": return tr("Delivery refused"); case "delivery_unknown": return tr("Delivery unknown");
    case "task_backlog": return tr("Task in backlog"); case "task_todo": return tr("Task in To do");
    case "task_in_progress": return tr("Task in progress"); case "task_in_review": return tr("Task in review");
    case "task_done": return tr("Task done"); case "task_cancelled": return tr("Task cancelled");
    case "gap": return tr("Observation gap"); default: return tr("Observed point");
  }
};
export function ObservedPanel(props: { team: string; phase: number; pane?: ObservedPane | null; follow?: ObservedFollow }) {
  const follower = observedFollower(props.team, props.follow);
  const [open, setOpen] = createSignal(false), [mode, setMode] = createSignal<"time" | "members">("time"), [error, setError] = createSignal("");
  createEffect(() => follower.select(props.phase, open()));
  onCleanup(() => follower.close());
  const pane = () => open() && props.pane?.epoch === follower.epoch() && props.pane?.page.scope.team === props.team && props.pane?.page.scope.phase === props.phase ? props.pane : null;
  async function page(cursor: string | null) { try { await invoke("teams_page", { key: `observed:${props.team}`, cursor }); setError(""); } catch (cause) { setError(String(cause)); } }
  return <section class="observed-timing" data-observed-panel>
    <button type="button" class="btn small" aria-expanded={open()} onClick={() => setOpen(!open())}>{tr("Observed timing")}</button>
    <Show when={open()}>
      <nav aria-label={tr("Observed timing")}><button type="button" class="btn small" aria-pressed={mode() === "time"} onClick={() => setMode("time")}>{tr("Observed Time")}</button><button type="button" class="btn small" aria-pressed={mode() === "members"} onClick={() => setMode("members")}>{tr("Members")}</button></nav>
      <p class="dim">{tr("Recorded delivery and sampled task states. Implementation time is unavailable.")}</p>
      <Show when={follower.error() || error()}>{(message) => <p role="status">{message()}</p>}</Show>
      <Show when={pane()} fallback={<p role="status">{tr("Observation coverage is unavailable.")}</p>}>{(selected) => <>
        <p>{tr("Last verified observation: %1", time(selected().page.as_of_ms))}</p>
        <Show when={selected().page.more || selected().page.truncated}><p role="status">{tr("Partial observation window. This page does not cover the whole phase.")}</p></Show>
        <Show when={selected().page.reasons.includes("clock_discontinuity") || selected().page.reasons.includes("observer_restart") || selected().page.rows.some((f) => f.category === "gap")}><p role="status">{tr("Observation continuity is interrupted. No duration is inferred across the gap.")}</p></Show>
        <Show when={mode() === "time"} fallback={<ul><For each={selected().coverage}>{(unit) => <li>{tr("Task #%1", unit.key.node)} <span>{unit.key.binding?.member ?? tr("Unattributed")}</span> <span class="dim">{unit.coverage === "sampled" ? tr("Sampled points") : tr("Coverage unavailable")}</span></li>}</For></ul>}>
          <Show when={selected().page.rows.length} fallback={<p>{tr("No observations in this selected window.")}</p>}><ol><For each={selected().page.rows}>{(fact) => <li data-observed-fact={fact.seq}><time>{time(fact.observed_at_ms)}</time> <span>{tr("Task #%1", fact.key.node)}</span> <b>{category(fact.category)}</b><Show when={fact.event_at_ms !== null}><span class="dim">{tr("Recorded event: %1", time(fact.event_at_ms))}</span></Show></li>}</For></ol></Show>
        </Show>
        <footer><button type="button" class="btn small" onClick={() => void page(null)}>{tr("First page")}</button><button type="button" class="btn small" disabled={!selected().page.next} onClick={() => void page(selected().page.next)}>{tr("Next page")}</button></footer>
      </>}</Show>
    </Show>
  </section>;
}
