import { Show } from "solid-js";
import { setDetailTab } from "../../store/ui";
import { shellSession, type ActivitySource } from "../../store/activitySource";
import type { Session } from "./types";
import { tr } from "../../i18n";

/**
 * Before the first answer (zerus-bz54l): a shell session records no activity
 * and reads none, so it says so and offers Terminal, as Qt's empty Activity
 * pointed to it (ActivityView.cpp:1150); a read that does not answer in time
 * says so with Retry; otherwise the read is under way.
 */
export function ActivityPending(props: { source: ActivitySource; session: Session | undefined }) {
  return (
    <Show when={!(props.session && !props.source.readOnly && shellSession(props.session))} fallback={
      <div class="act-empty"><b>{tr("No recorded activity")}</b>
        <p class="muted">{tr("Activity is not recorded for shell sessions. Use Terminal to work in this shell.")}</p>
        <button type="button" class="btn small" onClick={() => setDetailTab("terminal")}>{tr("Open Terminal")}</button>
      </div>}>
      <Show when={props.source.stalled?.()} fallback={<div class="act-older hint">{tr("Reading activity…")}</div>}>
        <div class="act-empty"><b>{tr("Activity did not load")}</b>
          <p class="muted">{tr("The session's computer did not answer in time.")}</p>
          <button type="button" class="btn small" onClick={() => props.source.retry?.()}>{tr("Retry")}</button>
        </div>
      </Show>
    </Show>
  );
}
