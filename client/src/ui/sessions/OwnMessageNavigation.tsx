import { Show, onCleanup, onMount } from "solid-js";
import { tr } from "../../i18n";
import { keys, nativeText, onCommand } from "../../store/shortcuts";
import { Icon } from "../lib/icons";
import { ownMessageNavigation } from "./own-message-navigation";

/** The same compact chips as the approved composer toolbar's Latest slot. */
export function OwnMessageNavigation() {
  const previous = () => tr("Previous message from you"), next = () => tr("Next message from you");
  const tip = (back: boolean) => [back && ownMessageNavigation()?.loading() ? tr("Reading earlier messages…") : back ? previous() : next(),
    ...keys(back ? "activity.previousOwn" : "activity.nextOwn").map(key => nativeText(key))].join("\n");
  onMount(() => {
    const stops = [onCommand("activity.previousOwn", () => ownMessageNavigation()?.navigate(-1)),
      onCommand("activity.nextOwn", () => ownMessageNavigation()?.navigate(1))];
    onCleanup(() => stops.forEach(stop => stop()));
  });
  return <>
    <button type="button" class="tchip icon" title={tip(true)} aria-label={previous()} aria-busy={!!ownMessageNavigation()?.loading()}
      disabled={!ownMessageNavigation()?.previous()} onClick={() => ownMessageNavigation()?.navigate(-1)}>
      <Show when={ownMessageNavigation()?.loading()} fallback={<Icon name="arrow-up" size={14} />}><span class="cmp-progress" /></Show>
    </button>
    <button type="button" class="tchip icon" title={tip(false)} aria-label={next()}
      disabled={!ownMessageNavigation()?.next()} onClick={() => ownMessageNavigation()?.navigate(1)}><Icon name="arrow-down" size={14} /></button>
    <Show when={ownMessageNavigation()?.loading()}><span class="sr-only" role="status">{tr("Reading earlier messages…")}</span></Show>
  </>;
}
