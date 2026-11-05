import { For, Show, createSignal } from "solid-js";
import type { JSX } from "solid-js";
import { tr } from "../../i18n";
import { Icon } from "../lib/icons";
import type { DraftMember } from "./types";
import { memberRowErrors, type ComputerChoice, type NewTeamStore } from "../../store/teamsNew";
import "../styles/newteam.css";

export interface NewTeamProps {
  store: NewTeamStore;
  computers: ComputerChoice[];
  git: boolean;
  lead: DraftMember;
  onOpenSettings: () => void;
  onCreated?: (team: string) => void;
}

export function NewTeam(props: NewTeamProps): JSX.Element {
  const [lead, setLead] = createSignal<DraftMember>(props.lead);
  const folder = () => props.store.fields().folder;
  const name = () => props.store.fields().name;
  const goal = () => props.store.fields().goal;
  const draft = () => props.store.draftId();
  const members = () => (props.store.fields().members.length > 0 ? props.store.fields().members : [lead()]);
  const errors = (member: DraftMember) => memberRowErrors(member, props.computers);

  const setMember = (index: number, patch: Partial<DraftMember>) => {
    const next = members().map((member, at) => (at === index ? { ...member, ...patch } : member));
    if (index === 0) setLead({ ...lead(), ...patch });
    props.store.edit("members", next);
  };
  const start = async () => {
    await props.store.startPlanning({
      folder: folder(),
      name: name(),
      lead: { ...members()[0], folder: folder() },
    });
  };
  const create = async () => {
    const result = await props.store.createTeam();
    if (result.ok && props.onCreated) props.onCreated(props.store.draftId());
  };

  return (
    <section class="nt" aria-label={tr("New team")}>
      <header class="nt-head">
        <h2>{tr("New team")}</h2>
        <span class="grow" />
        <Show when={draft()}>
          <button type="button" class="btn ghost small" onClick={props.onOpenSettings}>
            {tr("Team settings")}
          </button>
        </Show>
        <button type="button" class="icon-btn small" aria-label={tr("Close")} onClick={props.store.close}>
          <Icon name="x" size={14} />
        </button>
      </header>

      <div class="nt-body">
        <label class="nt-field">
          <span>{tr("Name")}</span>
          <input value={name()} onInput={(event) => props.store.edit("name", event.currentTarget.value)} />
        </label>
        <label class="nt-field">
          <span>{tr("Folder")}</span>
          <input value={folder()} onInput={(event) => props.store.setFolder(event.currentTarget.value)} />
        </label>
        <Show when={!props.git}>
          <p class="nt-note">{tr("This folder is not a Git repository. Every member will share it; worktrees are off.")}</p>
        </Show>
        <label class="nt-field nt-goal">
          <span>{tr("Goal")}</span>
          <textarea rows={4} value={goal()} onInput={(event) => props.store.edit("goal", event.currentTarget.value)} />
        </label>

        <div class="nt-members">
          <div class="nt-members-head">
            <b>{tr("Members")}</b>
            <span class="grow" />
            <span class="nt-hint">{tr("Lead")}</span>
          </div>
          <For each={members()}>
            {(member, index) => (
              <div class="nt-row">
                <input
                  class="nt-role"
                  placeholder={tr("Role")}
                  value={member.role}
                  onInput={(event) => setMember(index(), { role: event.currentTarget.value })}
                />
                <select
                  class="nt-agent"
                  value={member.agent}
                  onChange={(event) => setMember(index(), { agent: event.currentTarget.value })}
                >
                  <For each={["claude", "codex", "kimi", "dsh", "opencode"]}>
                    {(agent) => <option value={agent}>{agent}</option>}
                  </For>
                </select>
                <select
                  class="nt-computer"
                  value={member.computer}
                  onChange={(event) => setMember(index(), { computer: event.currentTarget.value })}
                >
                  <option value="">{tr("Computer")}</option>
                  <For each={props.computers}>
                    {(computer) => <option value={computer.alias}>{computer.label}</option>}
                  </For>
                </select>
                <Show when={errors(member).length > 0}>
                  <span class="nt-error">
                    {errors(member)
                      .map((error) =>
                        error.code === "role" ? tr("Pick a role") : tr("Computer %1 is offline", error.alias),
                      )
                      .join(", ")}
                  </span>
                </Show>
              </div>
            )}
          </For>
        </div>

        <Show when={props.store.error()}>
          <p class="nt-error nt-block-error">{props.store.error()}</p>
        </Show>
        <Show when={props.store.created()}>
          <p class="nt-created">{tr("Team created.")}</p>
        </Show>
      </div>

      <footer class="nt-foot">
        <Show when={!draft()}>
          <button
            type="button"
            class="btn small"
            disabled={props.store.busy() || !folder().trim()}
            onClick={start}
          >
            {tr("Start planning")}
          </button>
        </Show>
        <Show when={draft()}>
          <button type="button" class="btn small" disabled={props.store.busy()} onClick={create}>
            {tr("Create team")}
          </button>
        </Show>
      </footer>
    </section>
  );
}
