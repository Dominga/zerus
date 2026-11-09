import { For, Show, useContext } from "solid-js";
import { tr } from "../../i18n";
import { Icon } from "../lib/icons";
import type { ActivityItem, ToolEvent } from "./types";
import { TOOL_OUTPUT, shownOutput, type ProcessLinks } from "./tool-output";
import { HighlightQuery, SearchHighlight } from "./SearchHighlight";

export { TOOL_OUTPUT, outputKey, shownOutput, type ProcessLinks } from "./tool-output";

/** An event's heading, View process link and as much of its output as shows. */
function OutputEvent(props: { event: ToolEvent; shown?: number; onMore: () => void; process?: ProcessLinks }) {
  const shown = () => props.shown ?? TOOL_OUTPUT.first;
  const text = () => shownOutput(props.event.detail(), shown());
  // Inside a shown search hit the query is marked in the output (zerus-2r73y); ordinary Activity stays plain.
  const query = useContext(HighlightQuery);
  return (
    <div class="act-tool-event">
      <div class="act-tool-event-head"><b>{props.event.label}</b><span class="act-time">{props.event.at}</span></div>
      <Show when={props.event.process && props.process?.linkable(props.event.process) ? props.event.process : undefined}>{(process) =>
        <button type="button" class="link act-tool-process" onClick={() => props.process?.open(process())}>{tr("View process")}</button>
      }</Show>
      <Show when={props.event.size > 0}>
        <pre class="act-tool-detail mono">{query() ? <SearchHighlight text={text()} query={query()} /> : text()}</pre>
        <Show when={text().length < props.event.size}>
          <div class="act-tool-more">
            <button type="button" class="link" onClick={() => props.onMore()}>{tr("Show more")}</button>
            <span class="muted">{tr("Showing %1 of %2 characters", text().length, props.event.size)}</span>
          </div>
        </Show>
      </Show>
    </div>
  );
}

/**
 * A tool, tools or keep-warm row (ActivityView.cpp:1105-1140): collapsed, its
 * title and one-line preview; expanded, each event with its output, read from
 * the event only while the row is open.
 */
export function ToolRow(props: {
  item: ActivityItem;
  open: boolean;
  onToggle: () => void;
  /** How much of event `index`'s output shows; undefined is the first part. */
  shown: (index: number) => number | undefined;
  onMore: (index: number) => void;
  process?: ProcessLinks;
}) {
  const it = () => props.item;
  const query = useContext(HighlightQuery);
  return (
    <div class="act-tools" id={`item-${it().id}`} classList={{ open: props.open, warn: !!it().warn, warm: it().kind === "warm", inherited: !!it().inherited }}>
      <button class="act-tools-head" onClick={() => props.onToggle()} aria-expanded={props.open}>
        <Icon name="chevron-right" size={12} class="chev" />
        <b>{it().title}</b>
        <Show when={it().kind === "tools"}><span class="muted">/ {it().body}</span></Show>
        <span class="act-time">{it().at}</span>
      </button>
      <Show when={it().kind === "tool" && !props.open}><div class="act-tool-cmd mono">{it().body}</div></Show>
      <Show when={props.open && it().lines}>
        <div class="act-tool-lines mono"><For each={it().lines}>{(l) => <div>{query() ? <SearchHighlight text={l} query={query()} /> : l}</div>}</For></div>
      </Show>
      <Show when={props.open && it().events}>
        <div class="act-tool-events">
          <For each={it().events}>{(event, index) =>
            <OutputEvent event={event} shown={props.shown(index())} onMore={() => props.onMore(index())} process={props.process} />
          }</For>
        </div>
      </Show>
    </div>
  );
}
