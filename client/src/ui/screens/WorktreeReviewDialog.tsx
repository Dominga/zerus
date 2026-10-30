import { For, Show, createEffect, createSignal, onCleanup, onMount } from "solid-js";
import { Icon } from "../lib/icons";
import { worktreeReviewDialog, worktreeReview, worktreeReviewLoading, worktreeReviewIssuing, worktreeReviewError, closeWorktreeReview, checkWorktreesAgain, prepareWorktreeRemoval, confirmWorktreeRemoval, cancelWorktreeConfirmation, type WorktreeConfirmation } from "../../store/worktreeCleanup";
const leaf = (path: string) => path.split(/[\\/]/u).filter(Boolean).at(-1) ?? path;
/** The mockup's worktree-cleanup and setup-cleanup-confirm, backed by the
 * existing guarded CLI review. Opening, selecting and inspecting never remove. */
export default function WorktreeReviewDialog() {
  let dialog!: HTMLDialogElement;
  const origin = worktreeReviewDialog()?.origin;
  const [checked, setChecked] = createSignal<Record<number, boolean>>({});
  const [branches, setBranches] = createSignal(false), [asking, setAsking] = createSignal(false);
  const [confirmation, setConfirmation] = createSignal<{ value: WorktreeConfirmation; origin: HTMLElement }>();
  const view = worktreeReview, busy = () => !!view()?.busy || worktreeReviewIssuing(), reading = worktreeReviewLoading;
  const chosen = () => view()?.rows.filter((r) => r.selectable && checked()[r.id]) ?? [];
  const title = () => worktreeReviewDialog()?.focus ? "Remove worktree" : "Clean up worktrees";
  createEffect(() => { const v = view(); if (!v) return; setChecked(Object.fromEntries(v.rows.map((r) => [r.id, r.checked]))); setBranches(false); });
  const ask = async (origin: HTMLElement) => { if (asking()) return; setAsking(true); try { const value = await prepareWorktreeRemoval(chosen().map((r) => r.id), branches()); if (value) setConfirmation({ value, origin }); } finally { setAsking(false); } };
  onMount(() => { dialog.showModal(); queueMicrotask(() => dialog.querySelector<HTMLButtonElement>("[autofocus]")?.focus()); });
  onCleanup(() => { dialog.close(); queueMicrotask(() => { if (origin?.isConnected && !origin.matches(":disabled")) origin.focus(); else document.querySelector<HTMLElement>(".inspector .insp-tabs [aria-selected=true]")?.focus(); }); });
  return <>
    <dialog ref={dialog} class="dlg su-dlg worktree-review" style={{ width: "820px" }} aria-labelledby="worktree-review-title" onCancel={(e) => { e.preventDefault(); if (!busy()) closeWorktreeReview(); }} onClose={closeWorktreeReview}>
      <div class="dlg-head"><Icon name="folder-git-2" size={18} class="accent" /><h2 id="worktree-review-title">{title()}</h2><span class="grow" /><button class="icon-btn" aria-label="Close" disabled={busy()} onClick={closeWorktreeReview}><Icon name="x" size={16} /></button></div>
      <div class="dlg-body"><p class="muted su-pre">Computer: {view()?.machine ?? worktreeReviewDialog()?.machine}{"\n"}{view()?.focus ? `Worktree: ${view()?.focus}` : `Repository: ${view()?.repository ?? ""}`}</p>
        <p class="hint">Removal uses git worktree remove without force. Ignored files in a removed folder, such as build output, are deleted with it. Blocked worktrees explain what to finish first.</p>
        <table class="su-table wc-table"><thead><tr><th /><th>Worktree</th><th>State</th><th>Details</th></tr></thead><tbody>
          <Show when={!reading()} fallback={<tr><td colspan="4" class="hint">Reviewing worktrees on {view()?.machine ?? worktreeReviewDialog()?.machine}…</td></tr>}><For each={view()?.rows}>{(row) =>
            <tr classList={{ blocked: !row.selectable }} title={row.path}><td><input type="checkbox" aria-label={`Select ${row.path}`} checked={!!checked()[row.id]} disabled={!row.selectable || busy() || reading()} onChange={(e) => setChecked({ ...checked(), [row.id]: e.currentTarget.checked })} /></td><td><div class="ell"><b>{leaf(row.path)}</b></div><div class="mono dim ell">{row.branch || "—"}</div></td><td><span class="wc-verdict" data-v={row.state}>{row.state}</span></td><td class="hint" title={row.details}>{row.details}</td></tr>
          }</For></Show>
        </tbody></table>
        <label class="check" title="Uses git branch -d after the worktree is removed. Branches with commits that are not merged are always kept."><input type="checkbox" checked={branches()} disabled={reading() || busy()} onChange={(e) => setBranches(e.currentTarget.checked)} />Also delete merged branches</label>
        <Show when={busy()}><progress aria-label="Removing worktrees" /></Show>
      </div>
      <div class="dlg-foot"><span class="hint grow" role="status">{reading() ? `Reviewing worktrees on ${view()?.machine ?? worktreeReviewDialog()?.machine}…` : worktreeReviewError() || view()?.summary}<Show when={view()?.unknown}> An unknown result will not be retried.</Show></span>
        <button class="btn" disabled={reading() || busy() || !view() || !!view()?.rows.length && view()!.rows.every((r) => ["Removed", "Forgotten", "Unknown"].includes(r.state))} onClick={() => void checkWorktreesAgain()}>Review again</button>
        <button class="btn" autofocus disabled={busy()} onClick={closeWorktreeReview}>Close</button>
        <button class="btn danger" disabled={reading() || busy() || asking() || !chosen().length} onClick={(e) => void ask(e.currentTarget)}>{chosen().length > 1 ? `Remove ${chosen().length} worktrees…` : "Remove selected…"}</button>
      </div>
    </dialog>
    <Show when={confirmation()}>{(c) => <WorktreeConfirm value={c().value} title={title()} origin={c().origin} cancel={() => { setConfirmation(undefined); void cancelWorktreeConfirmation().catch(() => {}); }} confirm={async (consent) => { const id = c().value.id; setConfirmation(undefined); await confirmWorktreeRemoval(id, consent); }} />}</Show>
  </>;
}
function WorktreeConfirm(props: { value: WorktreeConfirmation; title: string; origin: HTMLElement; cancel: () => void; confirm: (consent: boolean) => Promise<void> }) {
  let dialog!: HTMLDialogElement;
  const [consent, setConsent] = createSignal(false);
  const c = props.value;
  onMount(() => { dialog.showModal(); queueMicrotask(() => dialog.querySelector<HTMLButtonElement>("[autofocus]")?.focus()); });
  onCleanup(() => { dialog.close(); queueMicrotask(() => { if (props.origin.isConnected) props.origin.focus(); }); });
  return <dialog ref={dialog} class="dlg su-dlg" style={{ width: "520px" }} aria-labelledby="worktree-confirm-title" onCancel={(e) => { e.preventDefault(); props.cancel(); }}>
    <div class="dlg-head"><Icon name="triangle-alert" size={18} class="accent" /><h2 id="worktree-confirm-title">{props.title}</h2><span class="grow" /><button class="icon-btn" aria-label="Close" onClick={props.cancel}><Icon name="x" size={16} /></button></div>
    <div class="dlg-body"><div class="notice warn"><Icon name="triangle-alert" size={16} /><b>Remove {c.paths.length} {c.paths.length === 1 ? "worktree" : "worktrees"} on {c.machine}?</b></div>
      <ul class="cc-list mono"><For each={c.paths}>{(path) => <li>{path}</li>}</For></ul><ul class="cc-facts">
        <Show when={c.ignored.length}><li style={{ "white-space": "pre-line" }}>Ignored files deleted with them: {c.ignored.join("\n") }.</li></Show><Show when={c.missing}><li>Missing folders: only Git's record is removed.</li></Show>
        <li>{c.branches.length ? `Merged branches deleted: ${c.branches.join(", ")}.` : "All branches are kept."}</li><Show when={c.process_warning}><li>{c.process_warning}</li></Show><li>Each worktree is checked again before removal. This cannot be undone.</li>
      </ul><Show when={c.consent}><label class="check"><input type="checkbox" checked={consent()} onChange={(e) => setConsent(e.currentTarget.checked)} />{c.consent}</label></Show>
    </div><div class="dlg-foot"><span class="grow" /><button class="btn" autofocus onClick={props.cancel}>Cancel</button><button class="btn solid-danger" disabled={!!c.consent && !consent()} onClick={() => void props.confirm(consent())}>{c.button}</button></div>
  </dialog>;
}
