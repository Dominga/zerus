import { Show, createEffect, createMemo, createSignal, on, onCleanup } from "solid-js";
import { Icon } from "../lib/icons";
import { tr } from "../../i18n";
import * as bridge from "../../bridge";
import { errorText } from "../../store/actions";
import type { KeepWarmState } from "../../types";
import type { Session } from "./types";
import { ALWAYS, durationLabel, keepWarmOf, mergeKeepWarm, positionOf, remainingText, statusLine, tooltipLines } from "./keep-warm";

const record = (value: unknown): Record<string, unknown> => (value && typeof value === "object" ? value as Record<string, unknown> : {});

/**
 * Details > Session: Keep cache warm. A switch, and while it is on a 1 to 8
 * slider (8 = Always) with the remaining time; the host's reason when the
 * agent or account cannot keep its cache warm. `inspected` is the Details read.
 */
export function KeepWarmRow(props: { session: Session; inspected: unknown; online: boolean }) {
  const [confirmed, setConfirmed] = createSignal<KeepWarmState>();
  const [draft, setDraft] = createSignal<number>();
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal("");
  const [now, setNow] = createSignal(Date.now() / 1000);
  const timer = setInterval(() => setNow(Date.now() / 1000), 30_000);
  onCleanup(() => clearInterval(timer));
  // A confirmed change belongs to its session and conversation only.
  createEffect(on(() => `${props.session.key}\n${props.session.conversationId ?? ""}`, () => { setConfirmed(undefined); setDraft(undefined); setError(""); }, { defer: true }));

  const state = createMemo(() => mergeKeepWarm(props.session.keepWarm, keepWarmOf(record(props.inspected).keep_warm), confirmed()));
  const on_ = () => !!state()?.enabled;
  const supported = () => state()?.supported !== false;
  const reported = () => state() !== undefined;
  const position = () => draft() ?? positionOf(state(), now());
  const usable = () => props.online && reported() && !busy() && !!props.session.runId && !!props.session.conversationId;
  const contextTokens = () => Number(record(record(record(props.inspected).session_usage).context).used) || 0;
  const tooltip = () => tooltipLines(state(), contextTokens()).join("\n");
  const remaining = () => remainingText(state(), now());
  const line = () => statusLine(state(), now());

  async function change(choice: bridge.KeepWarmChoice) {
    const s = props.session;
    setBusy(true);
    setError("");
    try {
      const target = { computer: s.computer, name: s.name, run_id: s.runId ?? "", conversation_id: s.conversationId ?? "" };
      const result = keepWarmOf(await bridge.setKeepWarm(target, choice, state()?.revision ?? 0));
      if (result) setConfirmed(result);
    } catch (failure) {
      const text = errorText(failure);
      setError(text === "busy" ? tr("A keep-warm change is already in progress.") : tr(text));
    } finally {
      setBusy(false);
      setDraft(undefined);
    }
  }
  const choiceAt = (position: number): bridge.KeepWarmChoice => (position >= ALWAYS ? { kind: "always" } : { kind: "hours", hours: position });

  return <>
    <div class="irow kw-row" title={tooltip()}>
      <span class="irow-ico"><Icon name="zap" size={16} /></span>
      <span class="irow-val ell">{tr("Keep cache warm")}</span>
      <Show when={on_()}>
        <input type="range" class="kw-range" min="1" max={String(ALWAYS)} step="1" value={position()} aria-label={tr("Keep cache warm for")}
          aria-valuetext={durationLabel(position())} disabled={!usable()}
          onInput={(event) => setDraft(Number(event.currentTarget.value))}
          onChange={(event) => void change(choiceAt(Number(event.currentTarget.value)))} />
        <span class="dim nowrap kw-value">{durationLabel(position())}</span>
      </Show>
      <button type="button" role="switch" class="switch" classList={{ on: on_() }} aria-checked={on_()} aria-label={tr("Keep cache warm")}
        disabled={!usable() || (!on_() && !supported())} onClick={() => void change(on_() ? { kind: "off" } : choiceAt(position()))} />
    </div>
    <Show when={on_() && remaining()}><div class="hint kw-hint">{remaining()}</div></Show>
    <Show when={line()}><div class="hint kw-hint">{line()}</div></Show>
    <Show when={!reported() && props.online}><div class="hint kw-hint">{tr("Update Zerus on this computer to keep the prompt cache warm.")}</div></Show>
    <Show when={error()}><div class="hint kw-hint kw-error" role="alert">{error()}</div></Show>
  </>;
}
