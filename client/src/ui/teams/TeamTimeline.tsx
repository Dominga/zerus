import { For, Show } from "solid-js";
import { tr } from "../../i18n";
import type { DrawerPage } from "../../store/teamDrawers";
import "../styles/team-timeline.css";

export function recordedDate(atMs: number): { iso: string; text: string } | null {
  if (!Number.isSafeInteger(atMs)) return null;
  const date = new Date(atMs);
  if (!Number.isFinite(date.getTime())) return null;
  return { iso: date.toISOString(), text: date.toLocaleString() };
}

/** Ordinal journal marks, not duration bars. No task actions or transcript reads. */
export function TeamTimeline(props: { page: DrawerPage }) {
  return <section class="team-timeline" aria-label={tr("Recorded events")}>
    <p class="dim">{tr("Recorded events on this journal page, not measured work time.")}</p>
    <Show when={props.page.timeline} fallback={<p role="alert">{tr("Timeline coverage is unavailable. Read the page again.")}</p>}>
      {(coverage) => <>
        <p class="dim" data-timeline-coverage>{tr("%1 recorded events shown (limit %2).", coverage().count, coverage().limit)} <Show when={coverage().first_seq !== null}>{tr("Sequences %1 to %2.", coverage().first_seq ?? "", coverage().last_seq ?? "")}</Show></p>
        <p class="dim">{coverage().earlier_page ? tr("Earlier pages are not included.") : tr("This is the first journal page.")} {coverage().later_page ? tr("A later page is available.") : tr("No later page was reported for this read.")}</p>
        <Show when={coverage().clock_reversed}><p role="status">{tr("Recorded clocks moved backwards on this page. Events stay in journal sequence order.")}</p></Show>
        <ol class="team-timeline-events">
          <For each={props.page.rows} fallback={<li class="dim">{tr("No events on this page.")}</li>}>{(row) => {
            const stamp = () => recordedDate(row.at_ms);
            return <li data-timeline-seq={row.seq}>
              <header><span class="mono">#{row.seq}</span><Show when={stamp()} fallback={<span>{tr("Recorded time unavailable")}</span>}>{(at) => <time dateTime={at().iso}>{at().text}</time>}</Show><span>{row.by || tr("Unknown actor")}</span><span class="team-pill">{row.kind}</span></header>
              <pre class="team-drawer-text">{row.text || row.kind}</pre>
              <Show when={row.text_truncated}><span class="dim">{tr("Only the beginning of this text is shown.")}</span></Show>
            </li>;
          }}</For>
        </ol>
      </>}
    </Show>
  </section>;
}
