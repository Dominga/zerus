import { For, Show, createSignal, onCleanup, onMount } from "solid-js";
import type { JSX } from "solid-js";
import { tr } from "../../i18n";
import { Icon } from "../lib/icons";
import { sessionSwitchAccount } from "../../bridge";
import { accountsPage, loadAccounts } from "../../store/accounts";
import { accountOf, accountOptions, switchOutcome, switchRequest, switchable } from "../../store/accountSwitch";
import type { Session } from "./types";

/** The composer's account chip (Qt AccountSwitchChip): the session's account,
 * the same agent's signed-in accounts, a confirmation with Cancel focused,
 * then `hgs switch-account`; the receipt is shown in the chip. */
export function AccountSwitchChip(props: { session: Session; compact?: boolean }): JSX.Element {
  const [open, setOpen] = createSignal(false);
  const [target, setTarget] = createSignal<{ id: string; label: string }>();
  const [busy, setBusy] = createSignal(false);
  const [notice, setNotice] = createSignal("");
  let dialog!: HTMLDialogElement;
  let chip!: HTMLButtonElement;

  const provider = () => props.session.accountProvider || props.session.agent || "";
  const current = () => accountOf(provider(), props.session.accountId ?? "");
  const options = () => accountOptions(accountsPage(), props.session.computer, provider(), current());
  const label = () => options().find((option) => option.id === current())?.label || props.session.accountId || tr("Native account");
  const close = (focus = false) => {
    setOpen(false);
    setTarget(undefined);
    if (dialog?.open) dialog.close();
    if (focus) chip?.focus();
  };
  onMount(() => {
    const outside = (event: PointerEvent) => {
      if (open() && !dialog?.open && !(event.target as Element).closest(".acct-switch-pop")) setOpen(false);
    };
    addEventListener("pointerdown", outside, true);
    onCleanup(() => removeEventListener("pointerdown", outside, true));
  });
  const ask = (option: { id: string; label: string }) => {
    setOpen(false);
    setTarget(option);
    queueMicrotask(() => dialog?.showModal());
  };
  const move = async () => {
    const choice = target();
    if (!choice || busy()) return;
    setBusy(true);
    setNotice("");
    try {
      const receipt = await sessionSwitchAccount(props.session.name, choice.id, switchRequest());
      setNotice(switchOutcome(receipt, "", choice.id));
      close();
      void loadAccounts(true);
    } catch (error) {
      setNotice(switchOutcome({}, String(error), choice.id));
      close();
    } finally {
      setBusy(false);
    }
  };

  return (
    <Show when={switchable(provider())}>
      <span class="acct-switch">
        <button ref={chip} type="button" class="tchip" classList={{ icon: props.compact }} title={tr("Account: %1", label())}
          aria-haspopup="menu" aria-expanded={open()} onClick={() => { setOpen(!open()); if (!open()) void loadAccounts(); }}>
          <Icon name="accounts" size={13} /><Show when={!props.compact}><span class="ell">{label()}</span></Show>
        </button>
        <Show when={open()}>
          <div class="acct-switch-pop" role="menu" aria-label={tr("Accounts on this computer")}>
            <Show when={options().length > 0} fallback={<span class="dim acct-switch-empty">{tr("No other signed-in account.")}</span>}>
              <For each={options()}>
                {(option) => (
                  <button type="button" role="menuitem" class="acct-switch-row" disabled={option.id === current()}
                    onClick={() => ask({ id: option.id, label: option.label })}>
                    <span class="ell">{option.label || option.id}</span>
                    <Show when={option.id === current()}><Icon name="check" size={12} /></Show>
                    <Show when={option.id !== current()}><span class="dim">{option.usage.status}</span></Show>
                  </button>
                )}
              </For>
            </Show>
            <Show when={notice()}><p class="acct-switch-notice" role="status">{notice()}</p></Show>
          </div>
        </Show>
      </span>
      <Show when={target()}>
        {(choice) => (
          <dialog ref={dialog} class="dlg" aria-labelledby="acct-switch-title"
            onCancel={(event) => { event.preventDefault(); close(true); }} onClose={() => setTarget(undefined)}>
            <div class="dlg-head"><Icon name="accounts" size={18} /><h2 id="acct-switch-title">{tr("Move this session to another account?")}</h2></div>
            <div class="dlg-body">
              <p>{tr("Move this session from %1 to %2?", label(), choice().label || choice().id)}</p>
              <p class="muted">{tr("The agent stops and starts again in the same conversation, folder and name. A failed start returns to the old account; an unconfirmed one is never retried.")}</p>
            </div>
            <div class="dlg-foot"><span class="spacer" />
              <button type="button" class="btn" autofocus onClick={() => close(true)}>{tr("Cancel")}</button>
              <button type="button" class="btn primary" disabled={busy()} onClick={() => void move()}>{tr("Move account")}</button>
            </div>
          </dialog>
        )}
      </Show>
    </Show>
  );
}
