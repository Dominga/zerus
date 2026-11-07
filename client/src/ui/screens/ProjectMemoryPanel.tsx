import { For, Show, createEffect, untrack } from "solid-js";
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
      <p class="hint">{tr("Teams of this project get a shared workspace in this store. Turning memory off keeps the store.")}</p>
    </div>
  );
}
