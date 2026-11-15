// Adapted from WhiteWh's MemberCard AddMemberDialog; uses our pinned roles,
// account semantics, original request and expected revision, never fork ops.
import { For, Show, createSignal } from "solid-js";
import { tr } from "../../i18n";
import type { TeamView } from "../../store/teams";
import { MemberChoices } from "./MemberChoices";
import { TeamEditDialog } from "./TeamEditDialog";
import { supportedChoice, type MemberChoice } from "./member-edit";
import type { EditSubmit } from "./TeamEditors";
export function AddMemberDialog(props: { view: TeamView; busy: boolean; locked?: boolean; error: string; close: () => void; submit: EditSubmit }) {
  const roles = props.view.member_roles ?? [];
  const [role, setRole] = createSignal<typeof roles[number] | undefined>(roles[0]);
  const initial = (): MemberChoice => {
    const r = role();
    return supportedChoice({ agent: r?.agent || "claude", model: r?.model || "", effort: r?.effort || "", account: r?.account.mode === "named" && r.account.account ? { mode: "named", account: r.account.account } : { mode: "freest" } });
  };
  const [choice, setChoice] = createSignal(initial());
  return <TeamEditDialog title={tr("Add member")} action={tr("Add member")} busy={props.busy} disabled={!role() || props.locked} error={props.error} close={props.close}
    submit={() => props.submit("add_member", { role: role()!.id, ...choice() })}>
    <p>{tr("The new member launches in its own folder. Existing members keep their sessions and work.")}</p>
    <label>{tr("Role")}<select class="field" aria-label={tr("Role")} value={role()?.id ?? ""} disabled={props.busy} onChange={(e) => { setRole(roles.find((r) => r.id === e.currentTarget.value)); setChoice(initial()); }}>
      <For each={roles}>{(r) => <option value={r.id}>{r.name || r.id}</option>}</For>
    </select></label>
    <Show when={role()} keyed>{(_role) => <MemberChoices initial={initial()} onChange={setChoice} disabled={props.busy} />}</Show>
    <Show when={!roles.length}><p class="dim">{tr("Refresh the team to read its pinned roles.")}</p></Show>
  </TeamEditDialog>;
}
