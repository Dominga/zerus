import { Index, Show, createEffect, createMemo, createSignal, on, onCleanup } from "solid-js";
import { machineReconnect, machineRetryDelays, type RetryDelaysOutcome } from "../../bridge";
import { tr } from "../../i18n";
import { desktopService } from "../../store/desktopService";
import { machineConnection, secondsLeft, useCountdownClock } from "../../store/machineConnection";
import type { MachinesView } from "../../types";
import { Actions, Spinner, Status } from "../components/Status";
import { TextInput } from "../lib/TextField";
import { reconnectLine, reconnectOutcome, reconnectReady, reconnectShown, retryDelaysDraft } from "./machines-ui";

// Machines: the selected computer's connection with the time to its next
// automatic attempt and Reconnect now, and this computer's reconnect schedule.
// Both act on the existing connection supervisors only; nothing here starts,
// restarts or activates the background service, a source or an agent.

/** A click stays in flight until the supervisor's state moves on; this only bounds a lost report. */
const IN_FLIGHT_MS = 30_000;

/** The selected computer's connection, the time to its next automatic attempt and Reconnect now. */
export function ReconnectStatus(props: { computer: string }) {
  const conn = () => machineConnection(props.computer);
  const waiting = () => conn()?.status === "reconnecting" && conn()?.retry?.phase === "waiting";
  const now = useCountdownClock(waiting);
  const line = createMemo(() => reconnectLine(conn(), secondsLeft(conn()?.retry, now())));
  const [sent, setSent] = createSignal<string>();
  const [outcome, setOutcome] = createSignal<string | null>(null);
  const progress = () => `${conn()?.status}\n${conn()?.retry?.phase}\n${conn()?.retry?.serial}`;
  createEffect(on(progress, () => setSent(undefined), { defer: true }));
  createEffect(on(() => props.computer, () => { setSent(undefined); setOutcome(null); }, { defer: true }));
  createEffect(() => {
    if (sent() === undefined) return;
    const guard = setTimeout(() => setSent(undefined), IN_FLIGHT_MS);
    onCleanup(() => clearTimeout(guard));
  });
  const inFlight = () => sent() === props.computer;
  const serviceDown = () => desktopService.enabled() && !desktopService.state()?.connected;
  const reconnect = async () => {
    const computer = props.computer;
    const current = conn();
    if (!current || inFlight()) return;
    setOutcome(null);
    setSent(computer);
    try {
      const code = await machineReconnect(computer, current.incarnation);
      if (props.computer !== computer) return;
      if (code !== "requested") setSent(undefined);
      setOutcome(reconnectOutcome(code));
    } catch (error) {
      if (props.computer !== computer) return;
      setSent(undefined);
      setOutcome(tr("Could not reconnect: %1", String(error)));
    }
  };
  return (
    <div class="mach-retry">
      <div class="mach-retry-status" role="status" aria-live="polite">
        <Show when={line().busy || inFlight()}><Spinner /></Show>
        <span class="mach-retry-state">{line().state}</span>
        <Show when={line().detail}>{(detail) => <span class="muted tabular">{detail()}</span>}</Show>
        <Show when={reconnectShown(conn())}>
          <button class="btn small" disabled={!reconnectReady(conn()) || inFlight() || serviceDown()}
            title={serviceDown() ? tr("The background service is not connected.") : tr("Try to connect now instead of waiting for the next automatic attempt")}
            onClick={() => void reconnect()}>{tr("Reconnect now")}</button>
        </Show>
      </div>
      <Show when={outcome()}>{(text) => <Status kind="muted">{text()}</Status>}</Show>
    </div>
  );
}

function appliedNotice(outcome: RetryDelaysOutcome): { text: string; error: boolean } {
  const applied = outcome.applied;
  if (applied.error) return { text: tr("Saved. The background service could not apply the schedule now: %1", applied.error), error: true };
  if (applied.retry_delays?.source === "kept") return { text: tr("Saved. The background service keeps its previous schedule: %1", applied.retry_delays.diagnostic ?? ""), error: true };
  return { text: tr("Saved. Each machine uses the new schedule from its next wait."), error: false };
}

/** This computer's reconnect schedule: one set of delays for every machine it connects to. */
export function ReconnectSchedule(props: { view: MachinesView }) {
  const saved = () => props.view.retry_delays ?? null;
  const [draft, setDraft] = createSignal<string[]>();
  const values = () => draft() ?? (saved()?.delays_seconds ?? [1, 2, 5, 10]).map(String);
  const checked = createMemo(() => retryDelaysDraft(values()));
  const changed = () => draft() !== undefined && checked().delays?.join(",") !== saved()?.delays_seconds.join(",");
  const [busy, setBusy] = createSignal(false);
  const [notice, setNotice] = createSignal<{ text: string; error: boolean } | null>(null);
  const kept = () => (desktopService.state()?.connected ? desktopService.state()?.fleet_retry_delays : null)?.source === "kept";
  const labels = [tr("First retry"), tr("Second retry"), tr("Third retry"), tr("Then every")];
  const save = async (delays: [number, number, number, number] | null) => {
    setBusy(true);
    setNotice(null);
    try {
      const outcome = await machineRetryDelays(delays);
      setDraft(undefined);
      setNotice(appliedNotice(outcome));
    } catch (error) {
      setNotice({ text: String(error), error: true });
    } finally {
      setBusy(false);
    }
  };
  const edit = (index: number, value: string) => {
    const next = [...values()];
    next[index] = value;
    setDraft(next);
  };
  return (
    <section class="mach-section">
      <h3>{tr("Reconnect schedule")}</h3>
      <p class="muted mach-origin">{tr("For every machine this computer connects to. After a failed connection it waits the first delay, then the second and the third, then repeats the last one. Reconnect now skips the wait.")}</p>
      <Show when={saved()} fallback={<Status kind="muted">{tr("Update hgs on this computer to change the reconnect schedule.")}</Status>}>
        {(current) => (<>
          <div class="mach-retry-grid">
            <Index each={labels}>{(label, index) => (
              <label class="mach-retry-field">
                <span>{label()}</span>
                <span class="row">
                  <TextInput class="field tabular" type="number" min={1} max={3600} step={1} value={values()[index]} disabled={busy()}
                    aria-invalid={draft() !== undefined && !!checked().error} onInput={(e) => edit(index, e.currentTarget.value)} />
                  <span class="muted">{tr("sec")}</span>
                </span>
              </label>
            )}</Index>
          </div>
          <Show when={draft() !== undefined && checked().error}>{(error) => <span class="danger hint">{error()}</span>}</Show>
          <Actions>
            <button class="btn primary" disabled={busy() || !changed() || !!checked().error} onClick={() => void save(checked().delays!)}>{tr("Save schedule")}</button>
            <button class="btn" disabled={busy() || (current().source === "default" && draft() === undefined)} onClick={() => void save(null)}>{tr("Reset to defaults")}</button>
            <Show when={draft() !== undefined}><button class="btn ghost" disabled={busy()} onClick={() => setDraft(undefined)}>{tr("Discard changes")}</button></Show>
          </Actions>
          <span class="muted mach-retry-defaults">{tr("Defaults: %1, %2 and %3 seconds, then every %4 seconds", ...current().defaults)}</span>
          <Show when={current().source === "invalid"}>
            <Status kind="error">{tr("The saved schedule is not used, so the defaults apply: %1 Reset to defaults to replace it.", current().diagnostic ?? "")}</Status>
          </Show>
          <Show when={kept() && current().source !== "invalid"}>
            <Status kind="warn">{tr("The background service could not read the saved schedule and keeps its previous one.")}</Status>
          </Show>
          <Show when={busy()}><Status kind="busy">{tr("Saving…")}</Status></Show>
          <Show when={notice()}>{(n) => <Status kind={n().error ? "error" : "muted"}>{n().text}</Status>}</Show>
        </>)}
      </Show>
    </section>
  );
}
