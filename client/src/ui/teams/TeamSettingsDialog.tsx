import { For, Show, createSignal } from "solid-js";
import type { JSX } from "solid-js";
import { tr } from "../../i18n";
import { Icon } from "../lib/icons";
import { teamAction } from "../../bridge";
import type { TeamMemberView, TeamView } from "../../types";
import "../styles/newteam.css";

type TeamSettings = Record<string, unknown>;

/** Qt TeamSettingsDialog: the owner's iron rules, branches, landing, restart,
 * the build broker, phases and each member's iron rules, one `settings` call. */
export interface TeamSettingsDialogProps {
  team: TeamView;
  onClose: () => void;
  onSaved?: () => void;
}

function lines(value: unknown): string[] {
  return Array.isArray(value) ? value.filter((line): line is string => typeof line === "string") : [];
}

function text(value: unknown): string {
  return typeof value === "string" ? value : "";
}

export function TeamSettingsDialog(props: TeamSettingsDialogProps): JSX.Element {
  const current = props.team;
  const settings = current.settings as TeamSettings;
  const [rules, setRules] = createSignal(lines(settings.iron_rules).join("\n"));
  const [memberRules, setMemberRules] = createSignal<Record<string, string>>(
    Object.fromEntries(
      current.members
        .filter((member) => member.state !== "removed")
        .map((member) => [member.id, lines(member.iron_rules).join("\n")]),
    ),
  );
  const branch = settings.branch_per_task as { template?: string } | null | undefined;
  const [branchOn, setBranchOn] = createSignal(Boolean(branch?.template));
  const [template, setTemplate] = createSignal(branch?.template || "task/{id}");
  const [base, setBase] = createSignal(text(settings.base_branch));
  const [restart, setRestart] = createSignal(Boolean(settings.restart_crashed));
  const [land, setLand] = createSignal(Boolean(settings.land_allowed));
  const [broker, setBroker] = createSignal(Boolean(settings.build_broker));
  const [leadUnpause, setLeadUnpause] = createSignal(Boolean(settings.lead_removes_own_pause));
  const [acceptance, setAcceptance] = createSignal(settings.phase_acceptance === "owner");
  const [asking, setAsking] = createSignal(false);
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal("");

  const rulesChanged = () => lines(rules()).join("\n") !== lines(settings.iron_rules).join("\n");
  const memberChanged = (member: TeamMemberView) =>
    (memberRules()[member.id] ?? "") !== lines(member.iron_rules).join("\n");
  const changed = () => {
    const patch: TeamSettings = {};
    if (rulesChanged()) patch.iron_rules = rules().split("\n").map((line) => line.trim()).filter(Boolean);
    const beforeTemplate = branch?.template || "";
    const afterTemplate = branchOn() ? template().trim() : "";
    if (afterTemplate !== beforeTemplate) patch.branch_per_task = afterTemplate ? { template: afterTemplate } : null;
    if (base().trim() !== text(settings.base_branch)) patch.base_branch = base().trim();
    if (restart() !== Boolean(settings.restart_crashed)) patch.restart_crashed = restart();
    if (land() !== Boolean(settings.land_allowed)) patch.land_allowed = land();
    if (broker() !== Boolean(settings.build_broker)) patch.build_broker = broker();
    if (leadUnpause() !== Boolean(settings.lead_removes_own_pause)) patch.lead_removes_own_pause = leadUnpause();
    if (acceptance() !== (settings.phase_acceptance === "owner")) patch.phase_acceptance = acceptance() ? "owner" : "lead";
    return patch;
  };
  const members = () =>
    current.members
      .filter((member) => member.state !== "removed" && memberChanged(member))
      .map((member) => ({
        id: member.id,
        iron_rules: (memberRules()[member.id] ?? "").split("\n").map((line) => line.trim()).filter(Boolean),
      }));

  const save = async () => {
    const patch = changed();
    const memberPatch = members();
    if (Object.keys(patch).length === 0 && memberPatch.length === 0) {
      props.onClose();
      return;
    }
    setBusy(true);
    setError("");
    try {
      const outcome = await teamAction({
        team: current.id,
        team_name: current.name,
        action: { kind: "settings", settings: patch, members_update: memberPatch },
      });
      if (outcome.outcome === "done") {
        props.onSaved?.();
        props.onClose();
      } else {
        setError(outcome.outcome === "confirm" ? outcome.confirmation.text : outcome.detail);
      }
    } catch (cause) {
      setError(String(cause));
    } finally {
      setBusy(false);
    }
  };
  const requestSave = () => {
    if (!land() || Boolean(settings.land_allowed)) {
      void save();
      return;
    }
    setAsking(true);
  };

  return (
    <aside class="nt-settings" aria-label={tr("Team settings")}>
      <header class="nt-head">
        <h2>{tr("Team settings")}</h2>
        <span class="grow" />
        <button type="button" class="icon-btn small" aria-label={tr("Close")} onClick={props.onClose}>
          <Icon name="x" size={14} />
        </button>
      </header>
      <div class="nt-body">
        <h3>{tr("Team iron rules")}</h3>
        <p class="nt-note">{tr("Mandatory additions to project rules. Only you, or the lead quoting your verified word, may change them.")}</p>
        <textarea rows={4} value={rules()} onInput={(event) => setRules(event.currentTarget.value)} />

        <h3>{tr("Branches")}</h3>
        <label class="nt-check">
          <input type="checkbox" checked={branchOn()} onChange={(event) => setBranchOn(event.currentTarget.checked)} />
          {tr("A branch per task")}
        </label>
        <div class="nt-row">
          <input value={template()} onInput={(event) => setTemplate(event.currentTarget.value)} placeholder={tr("Branch name template")} />
          <input value={base()} onInput={(event) => setBase(event.currentTarget.value)} placeholder={tr("the lead's branch")} />
        </div>
        <p class="nt-note">{tr("The branch name: {id} is the task, {member} the member and {team8} the team's first eight characters")}</p>

        <h3>{tr("Landing")}</h3>
        <label class="nt-check">
          <input type="checkbox" checked={land()} onChange={(event) => setLand(event.currentTarget.checked)} />
          {tr("Let land stages land tasks (land_allowed)")}
        </label>

        <h3>{tr("Members")}</h3>
        <label class="nt-check">
          <input type="checkbox" checked={restart()} onChange={(event) => setRestart(event.currentTarget.checked)} />
          {tr("Restart a member whose session crashed")}
        </label>
        <For each={current.members.filter((member) => member.state !== "removed")}>
          {(member) => (
            <>
              <label class="nt-field">
                <span>{tr("%1: member iron rules", member.role || member.id)}</span>
                <textarea
                  rows={2}
                  value={memberRules()[member.id] ?? ""}
                  onInput={(event) => setMemberRules({ ...memberRules(), [member.id]: event.currentTarget.value })}
                />
              </label>
            </>
          )}
        </For>

        <h3>{tr("Builds")}</h3>
        <label class="nt-check">
          <input type="checkbox" checked={broker()} onChange={(event) => setBroker(event.currentTarget.checked)} />
          {tr("Build broker for members sharing a folder")}
        </label>

        <h3>{tr("Phases")}</h3>
        <label class="nt-check">
          <input type="checkbox" checked={leadUnpause()} onChange={(event) => setLeadUnpause(event.currentTarget.checked)} />
          {tr("The lead removes its own pause")}
        </label>
        <label class="nt-check">
          <input type="checkbox" checked={acceptance()} onChange={(event) => setAcceptance(event.currentTarget.checked)} />
          {tr("New phases require my acceptance")}
        </label>
        <Show when={error()}><p class="nt-error">{error()}</p></Show>
      </div>
      <Show when={asking()}>
        <div class="nt-confirm" role="dialog" aria-label={tr("Allow landing")}>
          <p>{tr("Let land stages land this team's tasks? A land stage runs the catalog's command and may merge or push.")}</p>
          <div class="nt-confirm-actions">
            <button type="button" class="btn ghost small" autofocus onClick={() => setAsking(false)}>{tr("Cancel")}</button>
            <button type="button" class="btn small" disabled={busy()} onClick={() => { setAsking(false); void save(); }}>{tr("Allow landing")}</button>
          </div>
        </div>
      </Show>
      <footer class="nt-foot">
        <button type="button" class="btn ghost small" onClick={props.onClose}>{tr("Cancel")}</button>
        <button type="button" class="btn small" disabled={busy()} onClick={requestSave}>{tr("Save")}</button>
      </footer>
    </aside>
  );
}
