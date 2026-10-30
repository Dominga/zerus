import { For, Show, createEffect, createMemo, createSignal, on } from "solid-js";
import { createVirtualizer } from "@tanstack/solid-virtual";
import { contentSearch, searchItems, searchStatus, activateSearchItem } from "../../store/contentSearch";
import { search, selectedKey } from "../../store/ui";
import { Icon } from "../lib/icons";
import { AgentBadge, MachineBadge } from "./SessionCard";

import { SearchHighlight } from "./SearchHighlight";
export function SearchResults() {
  let element: HTMLDivElement | undefined;
  const [current, setCurrent] = createSignal("");
  const list = createVirtualizer({ get count() { return searchItems().length; }, getScrollElement: () => element?.isConnected ? element : null,
    estimateSize: (index) => searchItems()[index]?.found ? 106 : 64, getItemKey: (index) => searchItems()[index]?.id ?? index, overscan: 6 });
  const virtualRows = createMemo(() => list.getVirtualItems());
  const keys = createMemo(() => virtualRows().map((row) => String(row.key)), [] as string[], { equals: (a, b) => a.length === b.length && a.every((id, index) => id === b[index]) });
  const positions = createMemo(() => new Map(virtualRows().map((row) => [String(row.key), row])));
  const byId = createMemo(() => new Map(searchItems().map((item) => [item.id, item])));
  createEffect(on(search, () => { setCurrent(""); element?.scrollTo({ top: 0 }); }));
  createEffect(on(() => searchItems().map((item) => `${item.id}:${!!item.found}`).join("|"), () => list.measure(), { defer: true }));
  const key = (event: KeyboardEvent) => {
    const items = searchItems(); let index = items.findIndex((item) => item.id === current());
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault(); index = Math.max(0, Math.min(items.length - 1, index + (event.key === "ArrowDown" ? 1 : -1)));
      const item = items[index]; if (item) { setCurrent(item.id); activateSearchItem(item); list.scrollToIndex(index, { align: "auto" }); }
    } else if (event.key === "Enter" && index >= 0) { event.preventDefault(); activateSearchItem(items[index]); }
  };
  return <div class="content-search">
    <div class="content-search-status">
      <span role="status" title={contentSearch.errors().join("\n")}>{searchStatus()}</span>
      <button class="icon-btn small" aria-label="Search again" title="Search again" disabled={contentSearch.busy() || [...search().trim()].length < 2} onClick={contentSearch.refresh}><Icon name="refresh-cw" size={12} /></button>
    </div>
    <div class="content-search-list sp-scroll" ref={(el) => { element = el; requestAnimationFrame(() => list._willUpdate()); }} role="listbox" aria-label="Matching sessions and messages" aria-activedescendant={current() ? `content-search-${searchItems().findIndex((item) => item.id === current())}` : undefined} tabindex="0" onKeyDown={key}>
      <Show when={searchItems().length} fallback={<Show when={!contentSearch.busy() && !contentSearch.errors().length}>
        <div class="empty"><Icon name="search" size={26} /><h3>No matching sessions</h3><p>Nothing matches “{search().trim()}” in names, IDs or messages with the current filters.</p></div>
      </Show>}>
        <div style={{ height: `${list.getTotalSize()}px`, position: "relative" }}>
          <For each={keys()}>{(id) => {
            const virtual = () => positions().get(id);
            const item = () => byId().get(id)!;
            return <Show when={item()}>{(row) => <button class="content-search-row" id={`content-search-${virtual()?.index ?? 0}`} role="option" tabindex="-1" aria-selected={current() === item().id || (!current() && selectedKey() === item().session.key)}
              style={{ transform: `translateY(${virtual()?.start ?? 0}px)`, height: `${virtual()?.size ?? 0}px` }}
              onClick={() => { setCurrent(item().id); activateSearchItem(item()); }} title={`${item().session.name}\n${item().found?.hit.snippet ?? ""}`}>
              <span class="content-search-title ell"><SearchHighlight text={item().session.title} query={search().trim()} /></span>
              <span class="content-search-badges"><AgentBadge agent={item().session.agent} /><MachineBadge alias={item().session.machine} computer={item().session.computer} /></span>
              <span class="content-search-meta ell">{[item().session.archiveId ? "Archive" : "", item().found ? item().found!.hit.role === "user" ? "You" : item().found!.hit.role === "assistant" ? "Agent" : "Tool" : item().session.model,
                item().found ? new Date(item().found!.hit.at * 1000).toLocaleString(undefined, { day: "numeric", month: "short", hour: "2-digit", minute: "2-digit" }) : ""].filter(Boolean).join(" / ")}</span>
              <Show when={item().found}><span class="content-search-excerpt"><SearchHighlight text={item().found!.hit.snippet} query={search().trim()} /></span></Show>
            </button>}</Show>;
          }}</For>
        </div>
      </Show>
    </div>
  </div>;
}
