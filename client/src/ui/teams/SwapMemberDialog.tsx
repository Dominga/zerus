import { For, Show, createEffect, createSignal, onCleanup, onMount } from "solid-js";
import { tr } from "../../i18n";
import { teamRoleModels } from "../../bridge";
import { catalog } from "../../store/catalog";
import { accountsPage, loadAccounts } from "../../store/accounts";
import { accountChoices } from "../../store/newTeamSources";
import { windowPort } from "./ProposalCards";
import { createModelCatalogs } from "./catalogControls";
import { SWAP_AGENTS, firstModel, swapChoice, swapMemberCall, swapOptions, swapSessions, type SwapChoice } from "./swapMember";
import type { ComputerId, TeamMemberView, TeamView } from "../../types";
import { TextInput } from "../lib/TextField";

// Task #218: Qt's swapDialog for the Team tab's member menu. A new session
// takes the agent its account's models, the model's efforts and the agent's
// accounts; an existing one adopts another live local session. The core's
// confirmation is asked in TeamConfirmDialog, so Cancel sends nothing.

export function SwapMemberDialog(props: { computer: ComputerId; team: TeamView; member: TeamMemberView; close: () => void }) {
  let dialog!: HTMLDialogElement;
  const initial = swapChoice(props.member);
  const [fresh, setFresh] = createSignal(true);
  const [agent, setAgent] = createSignal(initial.agent);
  const [model, setModel] = createSignal(initial.model);
  const [effort, setEffort] = createSignal(initial.effort);
  const [account, setAccount] = createSignal(initial.account);
  const [session, setSession] = createSignal("");
  const [error, setError] = createSignal("");
  const [tick, setTick] = createSignal(0);
  const catalogs = createModelCatalogs((computer, agentId, accountId) => teamRoleModels(computer, agentId, accountId), () => setTick((n) => n + 1));
  const role = () => props.member.role || props.member.id;
  const sessions = () => swapSessions(props.team, catalog.rows, catalog.order);
  const accounts = () => accountChoices(accountsPage());
  const catalogOf = () => catalogs.get({ computer: props.computer, agent: agent(), account: account() });
  const options = () => (tick(), swapOptions(agent(), model(), account(), catalogOf(), accounts(), effort()));
  const models = () => options().models;
  const efforts = () => options().efforts;
  const accountOptions = () => options().accounts;

  createEffect(() => {
    catalogs.request({ computer: props.computer, agent: agent(), account: account() });
  });
  onMount(() => {
    dialog.showModal();
    void loadAccounts();
    if (sessions().length) setSession(sessions()[0].name);
    queueMicrotask(() => dialog.querySelector<HTMLSelectElement>("#swap-agent")?.focus());
  });
  onCleanup(() => { if (dialog.open) dialog.close(); });

  const changeAgent = (value: string) => {
    setAgent(value);
    setAccount("");
    setModel(firstModel(value));
    setEffort("");
  };
  const changeModel = (value: string) => {
    setModel(value);
    if (!efforts().includes(effort())) setEffort("");
  };
  const choice = (): SwapChoice | undefined => {
    if (!fresh()) {
      const picked = sessions().find((found) => found.name === session());
      return picked ? { agent: "", model: "", effort: "", account: "", session: picked } : undefined;
    }
    return { agent: agent(), model: model(), effort: effort(), account: account() };
  };
  const canSubmit = () => fresh() ? !!agent() : !!session();
  const submit = async () => {
    const picked = choice();
    if (!picked || !canSubmit()) return;
    setError("");
    try {
      const outcome = await swapMemberCall(windowPort, { computer: props.computer, team: props.team.id, teamName: props.team.name }, props.member, picked);
      if (outcome === "cancelled") return;
      if (outcome.outcome === "failed" || outcome.outcome === "uncertain") { setError(outcome.detail); return; }
      props.close();
    } catch (cause) {
      setError(String(cause instanceof Error ? cause.message : cause));
    }
  };

  return (
    <dialog ref={dialog} class="dlg team-swap-dialog" aria-labelledby="team-swap-title" onCancel={(event) => { event.preventDefault(); props.close(); }}>
      <div class="dlg-head"><h2 id="team-swap-title">{tr("Swap %1", role())}</h2></div>
      <div class="dlg-body">
        <p class="team-confirm-text">{props.member.lead
          ? tr("The lead keeps its role and the team's plan; its open items stay in the inbox and the new session gets one brief.")
          : tr("%1 keeps its id, role, tasks, folder and branch. An open task goes to the new session with what was done so far.", role())}</p>
        <label class="team-swap-mode"><input type="radio" name="swap-mode" checked={fresh()} onChange={() => setFresh(true)} />{tr("A new session")}</label>
        <div class="team-swap-fields" classList={{ disabled: !fresh() }}>
          <label class="team-swap-field">{tr("Agent")}
            <select id="swap-agent" class="field" value={agent()} disabled={!fresh()} onChange={(event) => changeAgent(event.currentTarget.value)}>
              <For each={SWAP_AGENTS}>{(value) => <option value={value}>{value}</option>}</For>
            </select>
          </label>
          <label class="team-swap-field">{tr("Model")}
            <TextInput class="field mono" list="team-swap-models" placeholder={tr("default model")} value={model()} disabled={!fresh()}
              onInput={(event) => changeModel(event.currentTarget.value)} />
            <datalist id="team-swap-models"><For each={models()}>{(option) => <option value={option.id}>{option.label}</option>}</For></datalist>
          </label>
          <label class="team-swap-field">{tr("Effort")}
            <select class="field" value={effort()} disabled={!fresh()} onChange={(event) => setEffort(event.currentTarget.value)}>
              <For each={efforts()}>{(value) => <option value={value}>{value || tr("its effort")}</option>}</For>
            </select>
          </label>
          <label class="team-swap-field">{tr("Account")}
            <select class="field" value={account()} disabled={!fresh()} onChange={(event) => setAccount(event.currentTarget.value)}>
              <option value="">{tr("Default account")}</option>
              <For each={accountOptions()}>{(found) => (
                <option value={found.id}>{found.signedIn ? found.label || found.id : tr("%1 (signed out)", found.label || found.id)}</option>
              )}</For>
            </select>
          </label>
        </div>
        <label class="team-swap-mode"><input type="radio" name="swap-mode" checked={!fresh()} disabled={!sessions().length}
          onChange={() => setFresh(false)} />{tr("An existing session")}</label>
        <select class="field team-swap-session" aria-label={tr("An existing session")} disabled={fresh() || !sessions().length}
          value={session()} onChange={(event) => setSession(event.currentTarget.value)}>
          <For each={sessions()}>{(found) => <option value={found.name}>{found.name}</option>}</For>
        </select>
        <Show when={!sessions().length}><p class="dim">{tr("No other live session to adopt")}</p></Show>
        <Show when={error()}><p class="danger" role="alert">{error()}</p></Show>
      </div>
      <div class="dlg-foot"><span class="spacer" />
        <button type="button" class="btn" onClick={props.close}>{tr("Cancel")}</button>
        <button type="button" class="btn primary" disabled={!canSubmit()} onClick={() => void submit()}>{tr("Swap…")}</button>
      </div>
    </dialog>
  );
}
