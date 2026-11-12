import { Show, Suspense, lazy } from "solid-js";
import { sessionModels as models } from "../../store/sessionModels";
import { Icon } from "../lib/icons";
import "../styles/newsession.css";

const SettingsDialog = lazy(() => import("./ModelSettingsDialog"));

export function ModelSettingsButton() {
  let anchor: HTMLButtonElement | undefined;
  return <Show when={models.view()}>{(v) => <>
    <button ref={anchor} type="button" class="btn ghost cmp-model" aria-label={v().accessible} title={v().tooltip}
      aria-haspopup="dialog" aria-expanded={models.opened()} data-shortcut-context={models.opened() ? "none" : undefined}
      onClick={(e) => { e.currentTarget.focus({ preventScroll: true }); models.open(); }}
      on:keydown={(e) => { if (e.key === "Escape" && models.opened()) { e.preventDefault(); e.stopPropagation(); models.close(); } }}>
      <span class="ell cmp-model-name">{v().shownModel}</span><span class="dim cmp-model-suffix">{v().suffix}</span>
      <Show when={v().defaults}><Icon name="triangle-alert" size={12} class="cmp-model-warn" /></Show><Icon name="chevron-down" size={12} />
    </button>
    <Show when={models.opened()}><Suspense><SettingsDialog anchor={() => anchor} /></Suspense></Show>
  </>}</Show>;
}
