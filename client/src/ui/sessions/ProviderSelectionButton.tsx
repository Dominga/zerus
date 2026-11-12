import { createEffect, createSignal, Show, Suspense, lazy } from "solid-js";
import { sessionModels as models } from "../../store/sessionModels";
import { accountsPage } from "../../store/accounts";
import { accountOf, movable } from "../../store/accountSwitch";
import { accountNameForId } from "../provider-selection";
import { AgentLogo, Icon } from "../lib/icons";
import type { Session } from "./types";
import "../styles/newsession.css";

const ProviderDialog = lazy(() => import("./ProviderSelectionDialog"));

export function ProviderSelectionButton(props: { session: Session; compact?: boolean }) {
  let anchor: HTMLButtonElement | undefined;
  const [opened, setOpened] = createSignal(false);
  const v = () => models.view();
  const provider = () => props.session.accountProvider || props.session.agent || "";
  const current = () => accountOf(provider(), props.session.accountId ?? "");
  const accountName = () => accountNameForId(accountsPage(), props.session.computer, provider(), current()) || props.session.accountId || current();
  const close = () => { models.close(); setOpened(false); };
  const open = () => { if (v()) models.open(); setOpened(true); };
  createEffect(() => { if (opened() && v() && !models.opened()) setOpened(false); });
  return (
    <Show when={v() || movable(provider())}>
      <button ref={anchor} type="button" class="btn ghost cmp-model provider-selection-button"
        aria-label={`${accountName()}${v() ? `, ${v()!.accessible}` : ""}`} title={v() ? `${accountName()}\n${v()!.tooltip}` : accountName()}
        aria-haspopup="dialog" aria-expanded={opened()} data-shortcut-context={opened() ? "none" : undefined}
        onClick={(event) => { event.currentTarget.focus({ preventScroll: true }); open(); }}
        on:keydown={(event) => { if (event.key === "Escape" && opened()) { event.preventDefault(); event.stopPropagation(); close(); } }}>
        <AgentLogo agent={provider()} size={13} fallback={<Icon name="accounts" size={13} />} />
        <Show when={!props.compact}><span class="ell provider-selection-account-name">{accountName()}</span></Show>
        <Show when={v()}>{(view) => <><span class="ell cmp-model-name">{view().shownModel}</span><span class="dim cmp-model-suffix">{view().suffix}</span></>}</Show>
        <Icon name="chevron-down" size={12} />
      </button>
      <Show when={opened()}><Suspense><ProviderDialog session={props.session} anchor={() => anchor} close={close} /></Suspense></Show>
    </Show>
  );
}
