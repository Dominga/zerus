import { Show, createEffect, createSignal } from "solid-js";
import { Icon } from "../lib/icons";
import { tr } from "../../i18n";
import { notify } from "../../store/actions";
import { keepWarmAvailable, keepWarmHours, setKeepWarm } from "../../store/keepWarm";
import { SLIDER_MAX_HOURS, sliderHours, warmable } from "./keep-warm";
import type { Session } from "./types";

/**
 * Details' Keep cache warm row (Qt SessionsWindow.cpp:1621-1638, 4811-4817):
 * the bolt, the label, while it is on the 1-8 h slider and its hours, and the
 * switch. Enabled for the main context of a session with a conversation,
 * never an archived one; off removes the session's hours.
 */
export function KeepWarmRow(props: { session: Session; main: boolean }) {
  const hours = () => keepWarmHours(props.session);
  const enabled = () => props.main && warmable(props.session) && keepWarmAvailable();
  const active = () => props.main && warmable(props.session) && hours() > 0;
  // The slider keeps its last position while the switch is off, as Qt's does.
  const [slider, setSlider] = createSignal(sliderHours(hours()));
  createEffect(() => { const saved = hours(); if (saved > 0) setSlider(sliderHours(saved)); });
  const save = (value: number) => {
    void setKeepWarm(props.session, value).catch(() => notify("error", tr("Keep cache warm could not be saved."), true));
  };
  const tip = () => keepWarmAvailable()
    ? tr("Shortly before the provider's prompt cache expires, Zerus sends the idle agent a one-word service message, so the next turn reads the context from the cache. Each one uses a little of the account's limit; it is never shown as a reply.")
    : tr("Keep cache warm does not run while the background service manages sessions.");
  return <div class="irow keep-warm">
    <span class="irow-ico"><Icon name="zap" size={16} /></span>
    <span class="irow-val ell" title={tip()}>{tr("Keep cache warm")}</span>
    <Show when={active()}>
      <input type="range" class="kw-hours" min="1" max={SLIDER_MAX_HOURS} step="1" value={slider()} disabled={!enabled()}
        aria-label={tr("Stop warming after this long without a turn")} title={tr("Stop warming after this long without a turn")}
        onInput={(e) => { const value = Number(e.currentTarget.value); if (value !== slider()) { setSlider(value); save(value); } }} />
      <span class="irow-label tabular kw-limit">{tr("%n h", slider())}</span>
    </Show>
    <button type="button" role="switch" class="switch" classList={{ on: active() }} aria-checked={active()} aria-label={tr("Keep cache warm")}
      title={tip()} disabled={!enabled()} onClick={() => save(active() ? 0 : slider())} />
  </div>;
}
