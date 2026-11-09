import { createSignal } from "solid-js";
import * as bridge from "../bridge";
import { notify } from "./actions";
import { tr } from "../i18n";
import type { Outcome, SettingsCheckView, Target } from "../types";

// Task #7: a model or effort change the agent did not visibly take used to
// leave every composer dead until a message was sent from Terminal. hgs now
// confirms it from the agent's own evidence (its empty prompt showing the
// requested model and effort) on every send and each inspection; the
// composer's bar (ui/sessions/SettingsCheckBar.tsx) explains an unknown state
// and offers Check again, which also runs by itself while the bar shows, and
// Continue anyway, which the person confirms first and hgs takes only while
// Terminal shows an empty prompt with no dialog. Nothing is typed into the
// agent by either.

/** How often the bar checks again by itself while it shows. */
export const RECHECK_MS = 20_000;

export interface SettingsCheckPort {
  run(target: Target, accept: boolean): Promise<Outcome>;
  notice(tone: "error" | "info", text: string): void;
}

const named = (model: string, effort: string) => (effort ? tr("%1 with %2 effort", model, effort) : model);

/** The bar's text: what was asked, what the agent shows and what each action does. */
export function settingsCheckText(bar: SettingsCheckView): { title: string; detail: string; accept: string } {
  const requested = named(bar.requested_model || tr("a new model"), bar.requested_effort);
  return {
    title: tr("The agent has not shown the switch to %1 yet", requested),
    detail: bar.reason || (bar.shown_model ? tr("It shows %1.", named(bar.shown_model, bar.shown_effort)) : ""),
    accept: bar.at_prompt
      ? tr("Continue with what the agent shows now. Terminal is at an empty prompt, so your next message goes to the agent's composer, never into a dialog.")
      : tr("Terminal shows a dialog or unsent text: answer or close it there first. Nothing is typed into a dialog."),
  };
}

export function createSettingsCheck(port: SettingsCheckPort) {
  const [busy, setBusy] = createSignal(false);
  const [confirming, setConfirming] = createSignal(false);

  /**
   * Check again (`accept` false) or Continue anyway. `quiet` is the bar's own
   * timer: it says nothing unless the change is confirmed.
   */
  async function check(target: Target, accept: boolean, quiet = false): Promise<string | undefined> {
    if (busy()) return undefined;
    setBusy(true);
    try {
      const outcome = await port.run(target, accept);
      if (outcome.outcome !== "done") {
        if (!quiet) port.notice("error", outcome.detail || tr("The check did not finish; nothing was changed."));
        return undefined;
      }
      const state = String((outcome.receipt as { state?: unknown } | null)?.state ?? "");
      if (state === "confirmed" || state === "none") port.notice("info", tr("The agent shows the new model settings. You can send messages again."));
      else if (state === "accepted") port.notice("info", tr("Continuing with the settings the agent shows. You can send messages again."));
      else if (!quiet) port.notice("info", tr("The agent still does not show the requested model settings."));
      return state;
    } catch (reason) {
      if (!quiet) port.notice("error", reason instanceof Error ? reason.message : String(reason));
      return undefined;
    } finally {
      setBusy(false);
      setConfirming(false);
    }
  }

  return { busy, confirming, setConfirming, check };
}

export type SettingsCheckStore = ReturnType<typeof createSettingsCheck>;

export const settingsCheck = createSettingsCheck({
  run: (target, accept) => bridge.runAction(target, { kind: "settings_check", accept }),
  notice: (tone, text) => notify(tone, text, tone === "error"),
});
