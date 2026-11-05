import { Show, createEffect, createSignal } from "solid-js";
import type { JSX } from "solid-js";
import { tr } from "../../i18n";
import { Icon } from "../lib/icons";
import type { TeamSettings } from "./types";
import type { NewTeamStore } from "../../store/teamsNew";
import "../styles/newteam.css";

export interface TeamSettingsProps {
  store: NewTeamStore;
  onClose: () => void;
}

export function TeamSettings(props: TeamSettingsProps): JSX.Element {
  const view = () => props.store.view();
  const [form, setForm] = createSignal<TeamSettings>({});
  createEffect(() => {
    const current = view();
    if (current) setForm({ ...(current.settings as TeamSettings) });
  });

  const set = (key: string, value: unknown) => setForm({ ...form(), [key]: value });
  const save = async () => {
    const result = await props.store.saveSettings(form());
    if (result.ok) props.onClose();
  };
  return (
    <Show when={view()}>
      <aside class="nt-settings" aria-label={tr("Team settings")}>
        <header class="nt-head">
          <h2>{tr("Team settings")}</h2>
          <span class="grow" />
          <button type="button" class="icon-btn small" aria-label={tr("Close")} onClick={props.onClose}>
            <Icon name="x" size={14} />
          </button>
        </header>
        <div class="nt-body">
          <label class="nt-check">
            <input
              type="checkbox"
              checked={Boolean(form().branch_per_task)}
              onChange={(event) => set("branch_per_task", event.currentTarget.checked)}
            />
            {tr("A branch per task")}
          </label>
          <label class="nt-field">
            <span>{tr("Base branch")}</span>
            <input
              value={String(form().branch_base ?? "")}
              onInput={(event) => set("branch_base", event.currentTarget.value)}
            />
          </label>
          <label class="nt-check">
            <input
              type="checkbox"
              checked={Boolean(form().restart_crashed)}
              onChange={(event) => set("restart_crashed", event.currentTarget.checked)}
            />
            {tr("Restart a crashed member")}
          </label>
          <label class="nt-check">
            <input
              type="checkbox"
              checked={Boolean(form().land)}
              onChange={(event) => set("land", event.currentTarget.checked)}
            />
            {tr("Land stages")}
          </label>
          <label class="nt-check">
            <input
              type="checkbox"
              checked={form().phase_acceptance === "owner"}
              onChange={(event) => set("phase_acceptance", event.currentTarget.checked ? "owner" : "all")}
            />
            {tr("New phases require my acceptance")}
          </label>
          <label class="nt-check">
            <input
              type="checkbox"
              checked={Boolean(form().lead_removes_own_pause)}
              onChange={(event) => set("lead_removes_own_pause", event.currentTarget.checked)}
            />
            {tr("The lead removes its own pause")}
          </label>
          <Show when={props.store.error()}>
            <p class="nt-error">{props.store.error()}</p>
          </Show>
        </div>
        <footer class="nt-foot">
          <button type="button" class="btn small" disabled={props.store.busy()} onClick={save}>
            {tr("Save")}
          </button>
        </footer>
      </aside>
    </Show>
  );
}
