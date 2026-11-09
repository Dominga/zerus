import { For, Show, createEffect, createSignal, untrack } from "solid-js";
import { tr } from "../../i18n";
import { trText } from "../../text";
import { pickLocalFolder, projectMemory } from "../../bridge";
import { teamConfirm } from "../../store/teamConfirm";
import { createProjectMemory, statusText } from "../../store/projectMemory";
import { Icon } from "../lib/icons";
import "../styles/projectmemory.css";

// Task 126: Qt's ProjectMemoryPanel on Projects. One switch turns RustCodeLore
// on for the whole project (all its folders); the store folder shows with
// Browse (an empty store takes the folder, one with records moves) and Use
// default; a move is confirmed first, copies, verifies and keeps the old
// store. The status names the records, the size and the index state.

export function ProjectMemoryPanel(props: { project: string; enabled?: boolean }) {
  const memory = createProjectMemory({
    call: projectMemory,
    pickFolder: (start) => pickLocalFolder(start),
    confirmMove: (from, to, records) => teamConfirm.ask({
      name: "projectMemoryMove",
      title: tr("Move the project memory?"),
      text: tr("Zerus copies the store with its %1 records from %2 to %3, checks every file, and only then uses the new folder. The old store is kept; delete it yourself once you no longer need it.", records, from, to),
      action: tr("Move store"),
    }),
  });
  createEffect(() => { const next = props.project; untrack(() => memory.setProject(next)); });
  // Task #397: the search runs over the project's store and its team sub-stores.
  const [query, setQuery] = createSignal("");
  const search = () => void memory.search(query());
  const editable = () => memory.editable() && props.enabled !== false;
  return (
    <div class="proj-memory">
      <div class="row pm-head">
        <label class="check">
          <input type="checkbox" checked={!!memory.view()?.enabled} disabled={!editable()}
            onChange={(event) => { const on = event.currentTarget.checked; event.currentTarget.checked = !!memory.view()?.enabled; void memory.setEnabled(on); }} />
          {tr("Use RustCodeLore for this project")}
        </label>
        <span class="grow" />
        <button type="button" class="icon-btn small" aria-label={tr("Refresh")} title={tr("Refresh")} disabled={memory.loading()} onClick={() => void memory.reload()}><Icon name="refresh-cw" size={13} /></button>
      </div>
      <Show when={memory.view()}>{(view) => (
        <>
          <div class="row pm-store">
            <span class="dim">{tr("Store")}</span>
            <span class="mono pm-path selectable" title={view().store}>{view().store}</span>
            <Show when={view().default}><span class="muted">{tr("(default)")}</span></Show>
          </div>
          <div class="row pm-actions">
            <button type="button" class="btn small" disabled={!editable()} title={view().records > 0 ? tr("The store holds records: they move with it, checked, and the old store is kept.") : tr("Choose the folder of the store.")}
              onClick={() => void memory.browse()}><Icon name="folder" size={12} />{view().records > 0 ? tr("Move…") : tr("Browse…")}</button>
            <Show when={!view().default}>
              <button type="button" class="btn small ghost" disabled={!editable()} onClick={() => void memory.useDefault()}>{tr("Use default folder")}</button>
            </Show>
          </div>
          <p class="pm-status" role="status">{statusText(view())}</p>
          <Show when={!view().enabled && view().exists}><p class="hint">{tr("Memory is off; the store is kept as it is.")}</p></Show>
          <Show when={view().moved_from.length}>
            <p class="hint">{tr("Earlier stores, kept:")}</p>
            <ul class="pm-old"><For each={view().moved_from}>{(folder) => <li class="mono selectable">{folder}</li>}</For></ul>
          </Show>
        </>
      )}</Show>
      <Show when={memory.notice()}><p class="pm-notice" role="status">{memory.notice()}</p></Show>
      <Show when={memory.error()}>{(error) => <p class="pm-error" role="alert">{trText(error())}</p>}</Show>
      {/* Task #397: search the project's store and its team sub-stores; each hit
          names the store that holds it, so a team's records are found here. */}
      <div class="row pm-search">
        <input class="field" type="search" maxLength={500} placeholder={tr("Search this project's memory…")}
          aria-label={tr("Search this project's memory")} value={query()} disabled={!editable() || memory.searching()}
          onInput={(event) => setQuery(event.currentTarget.value)}
          onKeyDown={(event) => { if (event.key === "Enter") { event.preventDefault(); search(); } }} />
        <button type="button" class="btn small" disabled={!editable() || !query().trim() || memory.searching()} onClick={search}>
          {memory.searching() ? tr("Searching…") : tr("Search")}
        </button>
      </div>
      <Show when={memory.searchError()}>{(error) => <p class="pm-error" role="alert">{trText(error())}</p>}</Show>
      <Show when={memory.found()}>{(result) => (
        <div class="pm-results" role="region" aria-label={tr("Memory search results")}>
          <Show when={result().hits.length} fallback={<p class="hint">{tr("Nothing found in this project's memory.")}</p>}>
            <ul>
              <For each={result().hits}>{(hit) => (
                <li class="pm-hit" data-store={hit.store}>
                  <span class="pm-hit-kind">{hit.kind}</span>
                  <span class="ell pm-hit-title">{hit.title || hit.id}</span>
                  <span class="pm-hit-store" title={tr("The store that holds this record")}>{hit.store}</span>
                </li>
              )}</For>
            </ul>
          </Show>
        </div>
      )}</Show>
      {/* The model download lore starts with a search reports here; the notice
          is the owner's to dismiss. */}
      <Show when={memory.openNotes().length}>
        <div class="pm-model-notice" role="status">
          <b>{tr("Memory search model")}</b>
          <ul><For each={memory.openNotes()}>{(note) => <li>{note}</li>}</For></ul>
          <button type="button" class="icon-btn small" aria-label={tr("Dismiss")} title={tr("Dismiss")} onClick={() => memory.dismissNotes()}><Icon name="x" size={12} /></button>
        </div>
      </Show>
      <p class="hint">{tr("Teams of this project get a shared workspace in this store. Turning memory off keeps the store.")}</p>
    </div>
  );
}
