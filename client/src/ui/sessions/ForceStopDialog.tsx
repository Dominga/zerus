import { Show, createEffect, createSignal, onCleanup, onMount } from "solid-js";
import * as bridge from "../../bridge";
import { applyCleanup, cleanup, type ProgramRetirement } from "../../store/terminationCleanup";
import { notify } from "../../store/actions";
import { tr } from "../../i18n";
import { Icon } from "../lib/icons";
import { TerminationPrograms } from "./TerminationPrograms";
import { readForceOffer } from "./force-offer";
import { programReviewMessage } from "./program-review";

/** Only this fresh confirmation can consume the native Force offer. Closing
 * leaves the known normal-stop result; no timeout or error confirms Force. */
export function ForceStopDialog(props: { id: string; label: string; close(): void }) {
  // The parent supplies these through a non-keyed Show accessor. Closing it
  // invalidates that accessor, including in this dialog's cleanup callbacks.
  const id = props.id, label = props.label, close = props.close;
  let dialog!: HTMLDialogElement;
  let closed = false, consumed = false, deadline = 0;
  let timer: number | undefined;
  const [offer, setOffer] = createSignal<bridge.ProgramForceOffer>();
  const [error, setError] = createSignal(false);
  const [submitted, setSubmitted] = createSignal(false);
  const [pending, setPending] = createSignal(false);
  const [failure, setFailure] = createSignal("");
  const [expired, setExpired] = createSignal(false);
  const [unavailable, setUnavailable] = createSignal<ProgramRetirement>();
  const current = (value: bridge.ProgramForceOffer) => {
    const view = cleanup();
    return view.revision < value.revision || view.rows.some((row) => row.id === id && row.force_revision === value.revision);
  };
  createEffect(() => {
    const value = offer();
    if (value && !consumed && !current(value)) {
      const row = cleanup().rows.find((row) => row.id === id);
      setUnavailable(row?.program_retirement ?? "source_unavailable");
    }
  });
  const expireIfNeeded = () => {
    if (!closed && !consumed && offer() && performance.now() >= deadline) setExpired(true);
  };
  onMount(() => {
    dialog.showModal();
    window.addEventListener("focus", expireIfNeeded);
    document.addEventListener("visibilitychange", expireIfNeeded);
    const started = performance.now();
    void readForceOffer(id, { offer: bridge.terminationProgramForceOffer, cancel: bridge.terminationProgramForceCancel }, () => !closed)
      .then((value) => {
        if (closed || !value) return;
        // Anchor to BEFORE the request: IPC/discovery latency can shorten this
        // display budget, never extend or restart the original native budget.
        deadline = started + value.remaining_millis;
        setExpired(performance.now() >= deadline);
        setOffer(value);
        timer = window.setTimeout(() => { if (!closed && !consumed) setExpired(true); }, Math.max(0, deadline - performance.now()));
      })
      .catch(() => { if (!closed) setError(true); });
  });
  onCleanup(() => {
    closed = true; window.clearTimeout(timer);
    window.removeEventListener("focus", expireIfNeeded);
    document.removeEventListener("visibilitychange", expireIfNeeded);
    dialog.close();
    const value = offer();
    if (value && !consumed) void bridge.terminationProgramForceCancel(id, value.confirmation).then(applyCleanup).catch(() => {});
  });
  const confirm = () => {
    const value = offer();
    if (!value || consumed || expired() || unavailable()) return;
    // A throttled/late browser timer never makes a stale offer clickable.
    if (performance.now() >= deadline) { setExpired(true); return; }
    if (!current(value)) { setUnavailable("source_unavailable"); return; }
    const confirmation = value.confirmation;
    consumed = true;
    setSubmitted(true); setPending(true);
    void (async () => {
      try {
        const view = await bridge.terminationProgramForceConfirm(id, confirmation);
        applyCleanup(view);
        if (!closed) close();
      } catch (reason) {
        // Keep a refused/lost admission visible on this exact confirmation.
        // It never makes the consumed handle available for another click.
        if (!closed) setFailure(String(reason));
        else notify("error", String(reason), true);
      } finally { if (!closed) setPending(false); }
    })();
  };
  return <dialog ref={dialog} class="dlg" aria-labelledby="force-stop-title" onCancel={(event) => { if (pending()) event.preventDefault(); else close(); }}>
    <div class="dlg-head"><Icon name="triangle-alert" size={18} /><h2 id="force-stop-title">{tr("Force stop programs?")}</h2></div>
    <div class="dlg-body"><p>{tr("These programs did not stop normally. Force stop them before removing %1?", label)}</p>
      <Show when={offer()} fallback={<p class="hint">{error() ? tr("The program list changed or could not be checked. Review the working folder again.") : tr("Checking…")}</p>}>{(value) => <TerminationPrograms force programs={value().programs} />}</Show>
      <Show when={!submitted() && (expired() || unavailable())}><p class="dlg-error" role="alert">{programReviewMessage(unavailable() ?? (expired() ? "expired" : undefined)) ?? tr("The original program review is no longer current. Review the working folder again.")}</p></Show>
      <Show when={failure()}><p class="dlg-error" role="alert">{failure()}</p></Show>
    </div>
    <div class="dlg-foot"><span class="grow" /><button class="btn solid-danger" disabled={!offer() || submitted() || expired() || !!unavailable()} onClick={confirm}>{tr("Force stop")}</button><button class="btn primary" autofocus disabled={pending()} onClick={close}>{failure() || expired() || unavailable() ? tr("Close") : tr("Cancel")}</button></div>
  </dialog>;
}
