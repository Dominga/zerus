import { For, Show, createMemo, createResource, onCleanup, onMount } from "solid-js";
import { desktopService, retainedSourceDetails } from "../../store/desktopService";
import { tr } from "../../i18n";
import { loadLegacyProjects, legacyProjectRow } from "../../store/legacyProjects";

/** This dialog reads the already published native status and has no retry seam. */
export function MigrationReportDialog(props: { origin: HTMLElement; close(): void }) {
  const origin = props.origin;
  const details = createMemo(() => retainedSourceDetails(desktopService.state()));
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
      <Show when={details() || legacyRow()} fallback={<p>{tr("No migration report is available for this connection.")}</p>}>
        <For each={["settings", "creations", "capture", "localReads", "localReadsReport", "drafts", "draftsReport", "downloads", "downloadsReport", "worktreeCache", "worktreeCacheReport"] as const}>{(key) => <Show when={details()?.[key]}>{(value) => <section>
          <h3>{key === "settings" ? tr("Qt and Zerus settings") : key === "creations" ? tr("Saved creation report") : key === "capture" ? tr("Session creation journal") : key === "localReads" ? tr("Local read marks") : key === "drafts" ? tr("Drafts") : key === "draftsReport" ? tr("Historical drafts report") : key === "downloads" ? tr("Downloaded files") : key === "downloadsReport" ? tr("Historical downloaded files report") : key === "worktreeCache" ? tr("Saved folder catalogs") : key === "worktreeCacheReport" ? tr("Historical folder catalog report") : tr("Historical local read-mark report")}</h3>
          <p>{value().reason}</p>
          <p class="hint">{value().next}</p>
          <details><summary>{tr("Technical details")}</summary>
            <dl><dt>{tr("Reason code")}</dt><dd><code>{value().code || tr("None")}</code></dd>
              <dt>{tr("Report status")}</dt><dd>{value().phase ?? tr("Not created yet")}</dd></dl>
          </details>
        </section>}</Show>}</For>
        <Show when={legacyRow()}>{(row) => <section>
          <h3>{tr("Legacy project lists")}</h3>
          <p class="hint">{tr("Project lists are read once per verified computer. Only missing links are added; existing Zerus choices are kept.")}</p>
          <p>{row().status}: <For each={row().keys}>{(key, index) => <>{index() > 0 ? ", " : ""}<code>{key}</code></>}</For></p>
          <For each={row().reasons}>{(reason) => <><p>{reason.text}</p><p class="hint"><code>{reason.code}</code></p></>}</For>
        </section>}</Show>
      </Show>
    </div>
    <div class="dlg-foot"><span class="spacer" /><button ref={closeButton} type="button" class="btn primary" onClick={props.close}>{tr("Close")}</button></div>
  </dialog>;
}
