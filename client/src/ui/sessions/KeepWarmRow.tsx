import { Show, createEffect, createMemo, createSignal, on, onCleanup } from "solid-js";
import { Icon } from "../lib/icons";
import { tr } from "../../i18n";
import * as bridge from "../../bridge";
import { errorText } from "../../store/actions";
import type { KeepWarmState, Target } from "../../types";
import type { Session } from "./types";
import { ALWAYS, choiceAt, durationLabel, durationMark, keepWarmOf, mergeKeepWarm, positionOf, remainingText, statusLine, tooltipLines } from "./keep-warm";

const record = (value: unknown): Record<string, unknown> => (value && typeof value === "object" ? value as Record<string, unknown> : {});

/**
 * Details > Session: Keep cache warm. A switch, and while it is on a 1 to 8
 * slider (8 = Always) with the remaining time; the host's reason when the
 * agent or account cannot keep its cache warm. `inspected` is the Details read.
 *
 * A change shows at once and keeps every control enabled and in place while
 * the host confirms it (zerus-ua4bi): a choice made meanwhile waits and goes
 * next with the confirmed revision, the newest one winning; a refusal returns
 * the row to the confirmed state.
 */
export function KeepWarmRow(props: { session: Session; inspected: unknown; online: boolean }) {
  const [confirmed, setConfirmed] = createSignal<KeepWarmState>();
  /** The slider while it is dragged, before its change. */
  const [draft, setDraft] = createSignal<number>();
  /** The newest choice the host has not confirmed yet. */
  const [pending, setPending] = createSignal<bridge.KeepWarmChoice>();
  const [busy, setBusy] = createSignal(false);
  let queued: bridge.KeepWarmChoice | undefined;
  const [error, setError] = createSignal("");
  const [now, setNow] = createSignal(Date.now() / 1000);
  const timer = setInterval(() => setNow(Date.now() / 1000), 30_000);
  onCleanup(() => clearInterval(timer));
  // A change belongs to the computer, row, run and conversation it was made
  // for. Selecting another target retires any reply still on its way.
  let generation = 0;
  const target = () => `${props.session.computer}\n${props.session.key}\n${props.session.runId ?? ""}\n${props.session.conversationId ?? ""}`;
  createEffect(on(target, () => { generation++; queued = undefined; setConfirmed(undefined); setDraft(undefined); setPending(undefined); setError(""); setBusy(false); }, { defer: true }));

  const state = createMemo(() => mergeKeepWarm(props.session.keepWarm, keepWarmOf(record(props.inspected).keep_warm), confirmed()));
  const on_ = () => { const p = pending(); return p ? p.kind !== "off" : !!state()?.enabled; };
  const supported = () => state()?.supported !== false;
  const reported = () => state() !== undefined;
  const position = () => {
    const d = draft(), p = pending();
    if (d !== undefined) return d;
    if (p && p.kind !== "off") return p.kind === "always" ? ALWAYS : p.hours;
    return positionOf(state(), now());
  };
  const usable = () => props.online && reported() && !!props.session.runId && !!props.session.conversationId;
  const contextTokens = () => Number(record(record(record(props.inspected).session_usage).context).used) || 0;
  const tooltip = () => tooltipLines(state(), contextTokens()).join("\n");
  const remaining = () => remainingText(state(), now());
  const line = () => statusLine(state(), now());

  // A removed row retires its reply and drops its queued choice (zerus-zzuy0).
  onCleanup(() => { generation++; queued = undefined; });

  function choose(choice: bridge.KeepWarmChoice) {
    setDraft(undefined);
    setPending(choice);
    setError("");
    if (busy()) queued = choice;
    else {
      const s = props.session;
      void send(choice, { computer: s.computer, name: s.name, run_id: s.runId ?? "", conversation_id: s.conversationId ?? "" }, state()?.revision ?? 0);
    }
  }
  /**
   * One write for `request`, the target its first choice was made for. A
   * choice queued meanwhile goes to that same target with the revision this
   * write's reply confirmed, never a newer one seen since, so a change from
   * another device makes the host refuse it instead of being overwritten
   * (zerus-trjfi). Without a confirmed revision the queued choice is dropped.
   */
  async function send(choice: bridge.KeepWarmChoice, request: Target, revision: number) {
    const mine = ++generation;
    setBusy(true);
    let confirmedRevision: number | undefined;
    try {
      const result = keepWarmOf(await bridge.setKeepWarm(request, choice, revision));
      if (mine === generation && result) { setNow(Date.now() / 1000); setConfirmed(result); confirmedRevision = result.revision; }
    } catch (failure) {
      if (mine !== generation) return;
      // A refused change also drops the one waiting behind it: it was made over this one.
      queued = undefined;
      const text = errorText(failure);
      setError(text === "busy" ? tr("A keep-warm change is already in progress.") : tr(text));
    } finally {
      if (mine === generation) {
        const next = queued;
        queued = undefined;
        if (next && confirmedRevision !== undefined) void send(next, request, confirmedRevision);
        else { setBusy(false); setPending(undefined); }
      }
    }
  }

  return <>
    <div class="irow kw-row" title={tooltip()} aria-busy={busy() || undefined}>
      <span class="irow-ico"><Icon name="zap" size={16} /></span>
      <span class="irow-val ell">{tr("Keep cache warm")}</span>
      <Show when={on_()}>
        <input type="range" class="kw-range" min="1" max={String(ALWAYS)} step="1" value={position()} aria-label={tr("Keep cache warm for")}
          aria-valuetext={durationLabel(position())} disabled={!usable()}
          onInput={(event) => setDraft(Number(event.currentTarget.value))}
          onChange={(event) => choose(choiceAt(Number(event.currentTarget.value)))} />
        <span class="kw-value" aria-hidden="true" title={position() >= ALWAYS ? durationLabel(position()) : undefined}>{durationMark(position())}</span>
      </Show>
      <button type="button" role="switch" class="switch" classList={{ on: on_() }} aria-checked={on_()} aria-label={tr("Keep cache warm")}
        disabled={!usable() || (!on_() && !supported())} onClick={() => choose(on_() ? { kind: "off" } : choiceAt(position()))} />
    </div>
    <Show when={on_() && remaining()}><div class="hint kw-hint">{remaining()}</div></Show>
    <Show when={line()}><div class="hint kw-hint">{line()}</div></Show>
    <Show when={!reported() && props.online}><div class="hint kw-hint">{tr("Update Zerus on this computer to keep the prompt cache warm.")}</div></Show>
    <Show when={error()}><div class="hint kw-hint kw-error" role="alert">{error()}</div></Show>
  </>;
}
