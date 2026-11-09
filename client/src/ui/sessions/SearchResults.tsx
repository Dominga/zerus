import { For, Show, createEffect, createMemo, on } from "solid-js";
import { createVirtualizer } from "@tanstack/solid-virtual";
import { contentSearch, searchItems, searchStatus, activateSearchItem, searchCursor as current, setSearchCursor as setCurrent, type SearchItem } from "../../store/contentSearch";
import { searchCards, selectCreation, selectedCreation } from "../../store/creations";
import { agentKind } from "../../store/catalog";
import { computerOf } from "./creations-ui";
import type { CreationCard } from "../../types";
import { searchTooltip } from "../../store/content-search-model";
import { search, selectedKey, stateFilter } from "../../store/ui";
import { archive } from "../../store/archive";
import { Icon } from "../lib/icons";
import { AgentBadge, MachineBadge } from "./SessionCard";

import { dayTime } from "../lib/usage";
import { SearchHighlight } from "./SearchHighlight";
import { tr } from "../../i18n";
/** A result row: a session or message, or a pending creation (Qt's "#creation" rows, SessionSearch.cpp:287-305). */
type ResultRow = { id: string; item: SearchItem; card?: undefined } | { id: string; card: CreationCard; item?: undefined };

export function SearchResults() {
  let element: HTMLDivElement | undefined;
  // Pending creations whose details match follow the sessions, as in Qt.
  const rows = createMemo((): ResultRow[] => [...searchItems().map((item) => ({ id: item.id, item })), ...searchCards().map((card) => ({ id: `creation:${card.id}`, card }))]);
  const activate = (row: ResultRow) => { if (row.item) activateSearchItem(row.item); else selectCreation(row.card.id); };
  // Before a cursor is chosen, the selected session's (or creation's) first row is the one current
  // option of this single-select list (zerus-ge5xc).
  const fallback = createMemo(() => (current() ? undefined : rows().find((row) => (row.item ? selectedKey() === row.item.session.key : selectedCreation() === row.card.id))?.id));
  const chosen = (row: ResultRow) => current() === row.id || fallback() === row.id;
  const list = createVirtualizer({ get count() { return rows().length; }, getScrollElement: () => element?.isConnected ? element : null,
    estimateSize: (index) => rows()[index]?.item?.found ? 90 : 64, gap: 2, getItemKey: (index) => rows()[index]?.id ?? index, overscan: 6 });
  const virtualRows = createMemo(() => list.getVirtualItems());
  const keys = createMemo(() => virtualRows().map((row) => String(row.key)), [] as string[], { equals: (a, b) => a.length === b.length && a.every((id, index) => id === b[index]) });
  const positions = createMemo(() => new Map(virtualRows().map((row) => [String(row.key), row])));
  const byId = createMemo(() => new Map(rows().map((row) => [row.id, row])));
  createEffect(on(search, () => { element?.scrollTo({ top: 0 }); }));
  // Next and previous session (window-commands.ts) choose a row from outside the list.
  createEffect(on(current, (id) => {
    const index = id ? rows().findIndex((row) => row.id === id) : -1;
    if (index >= 0) list.scrollToIndex(index, { align: "auto" });
  }, { defer: true }));
  createEffect(on(() => rows().map((row) => `${row.id}:${!!row.item?.found}`).join("|"), () => {
    list.measure();
    // Existing nodes can retain their size when new results arrive. Re-measure
    // them after resetting the estimates, even if ResizeObserver has no change.
    requestAnimationFrame(() => { if (element?.isConnected) element.querySelectorAll<HTMLElement>(".content-search-row").forEach((node) => list.measureElement(node)); });
  }, { defer: true }));
  const key = (event: KeyboardEvent) => {
    const all = rows(); let index = all.findIndex((row) => row.id === current());
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault(); index = Math.max(0, Math.min(all.length - 1, index + (event.key === "ArrowDown" ? 1 : -1)));
      const row = all[index]; if (row) { setCurrent(row.id); activate(row); list.scrollToIndex(index, { align: "auto" }); }
    } else if (event.key === "Enter" && index >= 0) { event.preventDefault(); activate(all[index]); }
  };
  return <div class="content-search">
    <div class="content-search-status">
      <span role="status" title={contentSearch.errors().join("\n")}>{searchStatus()}</span>
      <button class="icon-btn small" aria-label={tr("Search again")} title={tr("Search again")} disabled={contentSearch.busy() || [...search().trim()].length < 2} onClick={contentSearch.refresh}><Icon name="refresh-cw" size={12} /></button>
    </div>
    <div class="content-search-list sp-scroll" ref={(el) => { element = el; requestAnimationFrame(() => list._willUpdate()); }} role="listbox" aria-label={tr("Matching sessions and messages")} aria-activedescendant={current() ? `content-search-${rows().findIndex((row) => row.id === current())}` : undefined} tabindex="0" onKeyDown={key}>
      <Show when={rows().length} fallback={
        // zerus-080n8: while the archive scan is partial, nothing is known to be missing yet.
        <Show when={stateFilter() === "archived" && archive.partial()} fallback={<Show when={!contentSearch.busy() && !contentSearch.errors().length}>
          <div class="empty"><Icon name="search" size={26} /><h3>{tr("No matching sessions")}</h3><p>Nothing matches “{search().trim()}” in names, IDs or messages with the current filters.</p></div>
        </Show>}>
          <div class="empty"><Icon name="search" size={26} /><h3>{tr("%1 shown, %2 searched so far", 0, archive.searched())}</h3><p>{archive.partialNewer() ? tr("Newer continues the search.") : tr("Older continues the search.")}</p></div>
        </Show>
      }>
        <div style={{ height: `${list.getTotalSize()}px`, position: "relative" }}>
          <For each={keys()}>{(id) => {
            const virtual = () => positions().get(id);
            const row = () => byId().get(id)!;
            const card = () => row()?.card;
            return <Show when={row()?.item} fallback={<Show when={card()}>{(c) => <button class="content-search-row search-hit creation-hit" classList={{ selected: chosen(row()) }} data-index={virtual()?.index} ref={(el) => requestAnimationFrame(() => { if (el.isConnected) list.measureElement(el); })} id={`content-search-${virtual()?.index ?? 0}`} role="option" tabindex="-1" aria-selected={chosen(row())}
              style={{ transform: `translateY(${virtual()?.start ?? 0}px)` }}
              onClick={() => { setCurrent(row().id); activate(row()); }} title={[c().tag, c().caption, c().error].filter(Boolean).join("\n")}>
              <div class="row"><b class="ell"><SearchHighlight text={c().tag} query={search().trim()} /></b><span class="grow" /><AgentBadge agent={agentKind(c().cmd)} narrow /><MachineBadge alias={computerOf(c())} computer={c().host} /></div>
              <span class="content-search-meta sh-meta ell">{c().caption}</span>
              <Show when={c().error}><span class="content-search-excerpt sh-snippet"><SearchHighlight text={c().error} query={search().trim()} /></span></Show>
            </button>}</Show>}>{(found) => { const item = () => found(); return <button class="content-search-row search-hit" classList={{ selected: chosen(row()) }} data-index={virtual()?.index} ref={(el) => requestAnimationFrame(() => { if (el.isConnected) list.measureElement(el); })} id={`content-search-${virtual()?.index ?? 0}`} role="option" tabindex="-1" aria-selected={chosen(row())}
              style={{ transform: `translateY(${virtual()?.start ?? 0}px)` }}
              onClick={() => { setCurrent(item().id); activateSearchItem(item()); }} title={searchTooltip(item())}>
              <div class="row"><b class="ell"><SearchHighlight text={item().session.title} query={search().trim()} /></b><span class="grow" /><AgentBadge agent={item().session.agent} narrow /><MachineBadge alias={item().session.machine} computer={item().session.computer} /></div>
              <span class="content-search-meta sh-meta ell">{[item().session.archiveId ? "Archive" : "", item().found ? item().found!.hit.role === "user" ? "You" : item().found!.hit.role === "assistant" ? "Agent" : "Tool" : item().matched ?? item().session.model,
                item().found ? dayTime(item().found!.hit.at) : ""].filter(Boolean).join(" / ")}</span>
              <Show when={item().found}><span class="content-search-excerpt sh-snippet"><SearchHighlight text={item().found!.hit.snippet} query={search().trim()} /></span></Show>
            </button>}}</Show>;
          }}</For>
        </div>
      </Show>
    </div>
  </div>;
}
