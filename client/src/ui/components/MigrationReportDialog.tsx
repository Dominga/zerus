import { For, Show, createMemo, createResource, onCleanup, onMount } from "solid-js";
import { tr } from "../../i18n";
import { loadLegacyProjects, legacyProjectRow } from "../../store/legacyProjects";

/** This dialog reads the already published native status and has no retry seam. */
export function MigrationReportDialog(props: { origin: HTMLElement; close(): void }) {
  const origin = props.origin;
  const [legacy] = createResource(() => loadLegacyProjects().catch(() => undefined));
  const legacyRow = createMemo(() => legacyProjectRow(legacy()));
  let dialog!: HTMLDialogElement, closeButton!: HTMLButtonElement;
  onMount(() => { dialog.showModal(); closeButton.focus(); });
  onCleanup(() => {
    if (dialog.open) dialog.close();
    queueMicrotask(() => { if (origin.isConnected) origin.focus(); });
  });
  return <dialog ref={dialog} class="dlg" style={{ width: "600px" }} aria-labelledby="migration-report-title" onCancel={props.close} onClose={props.close}>
    <div class="dlg-head"><h2 id="migration-report-title">{tr("Migration report")}</h2></div>
    <div class="dlg-body">
      <Show when={legacyRow()} fallback={<p>{tr("No migration report is available for this connection.")}</p>}>
        {(row) => <section>
          <h3>{tr("Legacy project lists")}</h3>
          <p class="hint">{tr("Project lists are read once per verified computer. Only missing links are added; existing Zerus choices are kept.")}</p>
          <p>{row().status}: <For each={row().keys}>{(key, index) => <>{index() > 0 ? ", " : ""}<code>{key}</code></>}</For></p>
          <For each={row().reasons}>{(reason) => <><p>{reason.text}</p><p class="hint"><code>{reason.code}</code></p></>}</For>
        </section>}
      </Show>
    </div>
    <div class="dlg-foot"><span class="spacer" /><button ref={closeButton} type="button" class="btn primary" onClick={props.close}>{tr("Close")}</button></div>
  </dialog>;
}
