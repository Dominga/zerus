// Field layout and dependent-choice resets adapted from WhiteWh's add/swap
// dialogs. Our launch writer decides the account; this form grants no effects.
import { For, Show, createMemo, createSignal, createUniqueId, onMount } from "solid-js";
import { accountsPage, loadAccounts } from "../../store/accounts";
import { tr } from "../../i18n";
import { TextInput } from "../lib/TextField";
import { MEMBER_AGENTS, memberAccounts, type MemberChoice } from "./member-edit";
export function MemberChoices(props: { initial: MemberChoice; disabled: boolean; onChange: (choice: MemberChoice) => void }) {
  const [agent, setAgent] = createSignal(props.initial.agent);
  const [model, setModel] = createSignal(props.initial.model);
  const [effort, setEffort] = createSignal(props.initial.effort);
  const [account, setAccount] = createSignal(props.initial.account.mode === "named" ? props.initial.account.account : "");
  const id = createUniqueId();
  const accounts = createMemo(() => memberAccounts(accountsPage(), agent()));
  const changed = () => props.onChange({ agent: agent(), model: model(), effort: effort(), account: account() ? { mode: "named", account: account() } : { mode: "freest" } });
  onMount(() => { void loadAccounts().catch(() => undefined); });
  return <div class="team-member-edit-fields">
    <label>{tr("Agent")}<select class="field" aria-label={tr("Agent")} value={agent()} disabled={props.disabled} onChange={(e) => { setAgent(e.currentTarget.value); setAccount(""); setModel(""); setEffort(""); changed(); }}>
      <For each={MEMBER_AGENTS}>{(a) => <option value={a}>{a === "dsh" ? "DeepSeek" : a}</option>}</For>
    </select></label>
    <label>{tr("Model")}<TextInput class="field mono" aria-label={tr("Model")} value={model()} maxlength={256} placeholder={tr("default model")} disabled={props.disabled}
      onInput={(e) => { setModel(e.currentTarget.value); changed(); }} /></label>
    <label>{tr("Effort")}<TextInput class="field" aria-label={tr("Effort")} value={effort()} maxlength={64} placeholder={tr("default effort")} disabled={props.disabled}
      onInput={(e) => { setEffort(e.currentTarget.value); changed(); }} /></label>
    <label>{tr("Account")}<select class="field" aria-label={tr("Account")} value={account()} disabled={props.disabled} onChange={(e) => { setAccount(e.currentTarget.value); setModel(""); setEffort(""); changed(); }}>
      <option value="">{tr("The freest account")}</option>
      <Show when={account() && !accounts().some((a) => a.id === account())}><option value={account()}>{tr("%1 (not in the current catalog)", account())}</option></Show>
      <For each={accounts()}>{(a) => <option value={a.id} disabled={!a.signedIn}>{a.signedIn ? a.label : tr("%1 (signed out)", a.label)}</option>}</For>
    </select></label>
    <p class="dim" id={id}>{tr("Leave model and effort empty to use the selected account's defaults.")}</p>
  </div>;
}
