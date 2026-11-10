import { Show, createEffect, onCleanup } from "solid-js";
import { RECHECK_MS, settingsCheck as defaultStore, settingsCheckText, type SettingsCheckStore } from "../../store/settingsCheck";
import { tr } from "../../i18n";
import type { SettingsCheckView, Target } from "../../types";
import type { Session } from "./types";
import { Icon } from "../lib/icons";

/** The session a composer's settings check goes to: its computer, name and exact run and conversation. */
export function settingsTarget(s: Pick<Session, "computer" | "name" | "runId" | "conversationId"> | undefined): Target | undefined {
  return s && s.runId ? { computer: s.computer, name: s.name, run_id: s.runId, conversation_id: s.conversationId ?? "" } : undefined;
}

/**
 * Task #7: shown above the editor of every desktop composer (session view,
 * Activity, the Team tab and New team's lead conversation) while the agent has
 * not visibly taken a model or effort change. It says what was asked and what
 * the agent shows, checks again by itself while it shows, and offers Check
 * again and Continue anyway. Continue anyway asks first, inline, and is
 * offered only while Terminal shows an empty prompt with no dialog; hgs checks
 * that again before it accepts. Neither action types into the agent.
 */
export function SettingsCheckBar(props: { bar: SettingsCheckView | null | undefined; session: Session | undefined; store?: SettingsCheckStore; recheckMs?: number }) {
  const store = () => props.store ?? defaultStore;
  const target = () => settingsTarget(props.session);
  createEffect(() => {
    const shown = props.bar, to = target();
    if (!shown || !to) return;
    const timer = setInterval(() => { void store().check(to, false, true); }, props.recheckMs ?? RECHECK_MS);
    onCleanup(() => clearInterval(timer));
  });
  return (
    <Show when={props.bar && target() ? props.bar : undefined}>{(bar) => {
      const text = () => settingsCheckText(bar());
      return (
        <div class="act-queue settings-check" role="status" aria-label={tr("Model settings not confirmed")} data-settings-check={bar().at_prompt ? "at-prompt" : "dialog"}>
          <Icon name="clock" size={14} />
          <span class="settings-check-text">
            <b>{text().title}</b>
            <Show when={text().detail}><span class="dim">{text().detail}</span></Show>
          </span>
          <Show when={!store().confirming()} fallback={
            <span class="settings-check-confirm" data-settings-confirm>
              <span class="ell" title={text().accept}>{text().accept}</span>
              <button type="button" class="btn small primary" data-settings-accept disabled={store().busy() || !bar().at_prompt}
                onClick={() => { const to = target(); if (to) void store().check(to, true); }}>{tr("Continue")}</button>
              <button type="button" class="btn small" onClick={() => store().setConfirming(false)}>{tr("Cancel")}</button>
            </span>
          }>
            <button type="button" class="btn small" data-settings-recheck disabled={store().busy()}
              title={tr("Read the agent's model and effort again; nothing is typed")}
              onClick={() => { const to = target(); if (to) void store().check(to, false); }}>{tr("Check again")}</button>
            <button type="button" class="btn small" data-settings-continue disabled={store().busy() || !bar().at_prompt} title={text().accept}
              onClick={() => store().setConfirming(true)}>{tr("Continue anyway…")}</button>
          </Show>
        </div>
      );
    }}</Show>
  );
}
