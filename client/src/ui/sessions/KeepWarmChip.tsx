import { Show, createMemo, createSignal, onCleanup } from "solid-js";
import { steadyTitle } from "../lib/steady-title";
import { Icon } from "../lib/icons";
import { followedFor } from "../../store/actions";
import { headerKeepWarm } from "./keep-warm";
import { contextTokensOf, currentKeepWarm, revealKeepWarm } from "./keep-warm-state";
import type { Session } from "./types";

/**
 * The session header's keep-warm mark (zerus-yacns), on the chip row beside
 * the model label: the bolt and the time left, or an infinity sign for Always,
 * while keep-warm is on. It reads only the listing, the Details summary of the
 * followed session and a change Details confirmed. One node for as long as it
 * is on: refreshes and the clock change only its text and tooltip. A click
 * opens Details at the Keep cache warm control.
 */
export function KeepWarmChip(props: { session: Session }) {
  const [now, setNow] = createSignal(Date.now() / 1000);
  const timer = setInterval(() => setNow(Date.now() / 1000), 30_000);
  onCleanup(() => clearInterval(timer));
  const inspected = () => followedFor(props.session.key).value;
  const view = createMemo(() => (props.session.state === "archived" ? undefined : headerKeepWarm(currentKeepWarm(props.session, inspected()), now(), contextTokensOf(inspected()))));
  // Its time left changes each minute; the tooltip holds still under the pointer (zerus-5ryrx).
  const tip = steadyTitle(() => view()?.tooltip);
  return (
    <Show when={!!view()}>
      <button type="button" class="dh-warm" title={tip.title()} onPointerEnter={tip.enter} onPointerLeave={tip.leave} aria-label={view()?.label} onClick={revealKeepWarm}>
        <Icon name="zap" size={12} /><span>{view()?.mark}</span>
      </button>
    </Show>
  );
}
