import { For, Show, createMemo, onCleanup, onMount } from "solid-js";
import { desktopService, retainedSourceDetails } from "../../store/desktopService";
import { tr } from "../../i18n";

/** This dialog reads the already published native status and has no retry seam. */
export function MigrationReportDialog(props: { origin: HTMLElement; close(): void }) {
  const origin = props.origin;
  const details = createMemo(() => retainedSourceDetails(desktopService.state()));
  let dialog!: HTMLDialogElement, closeButton!: HTMLButtonElement;
  onMount(() => { dialog.showModal(); closeButton.focus(); });
  onCleanup(() => {
    if (dialog.open) dialog.close();
    queueMicrotask(() => { if (origin.isConnected) origin.focus(); });
  });
  return <dialog ref={dialog} class="dlg" style={{ width: "600px" }} aria-labelledby="migration-report-title" onCancel={props.close} onClose={props.close}>
    <div class="dlg-head"><h2 id="migration-report-title">{tr("Migration report")}</h2></div>
    <div class="dlg-body">
      <Show when={details()} fallback={<p>{tr("No migration report is available for this connection.")}</p>}>
        <For each={["settings", "creations", "capture", "localReads", "localReadsReport", "drafts", "worktreeCache"] as const}>{(key) => <Show when={details()?.[key]}>{(value) => <section>
          <h3>{key === "settings" ? tr("Qt and Zerus settings") : key === "creations" ? tr("Saved creation report") : key === "capture" ? tr("Session creation journal") : key === "localReads" ? tr("Local read marks") : key === "drafts" ? tr("Drafts") : key === "worktreeCache" ? tr("Saved folder catalogs") : tr("Historical local read-mark report")}</h3>
          <p>{value().reason}</p>
          <p class="hint">{value().next}</p>
          <details><summary>{tr("Technical details")}</summary>
            <dl><dt>{tr("Reason code")}</dt><dd><code>{value().code || tr("None")}</code></dd>
              <dt>{tr("Report status")}</dt><dd>{value().phase ?? tr("Not created yet")}</dd></dl>
          </details>
        </section>}</Show>}</For>
      </Show>
    </div>
    <div class="dlg-foot"><span class="spacer" /><button ref={closeButton} type="button" class="btn primary" onClick={props.close}>{tr("Close")}</button></div>
  </dialog>;
}
