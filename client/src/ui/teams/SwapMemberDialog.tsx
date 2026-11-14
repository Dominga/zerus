// Adapted from WhiteWh's SwapMemberDialog. The active cutover and adoption
// choices are deliberately absent: this package replaces only a stopped seat.
import { createSignal } from "solid-js";
import { tr } from "../../i18n";
import type { TeamMember } from "../../store/teams";
import { MemberChoices } from "./MemberChoices";
import { TeamEditDialog } from "./TeamEditDialog";
import { swapBlocked, type MemberChoice } from "./member-edit";
import type { EditSubmit } from "./TeamEditors";
export function SwapMemberDialog(props: { member: TeamMember; busy: boolean; locked?: boolean; error: string; close: () => void; submit: EditSubmit }) {
  const [choice, setChoice] = createSignal<MemberChoice>({ agent: props.member.agent, model: props.member.model || "", effort: props.member.effort || "", account: props.member.account_choice?.mode === "named" && props.member.account_choice.account ? { mode: "named", account: props.member.account_choice.account } : { mode: "freest" } });
  return <TeamEditDialog title={tr("Swap %1", props.member.id)} action={tr("Swap…")} busy={props.busy} disabled={props.locked || !!swapBlocked(props.member)} error={props.error || swapBlocked(props.member) || ""} close={props.close}
    submit={() => props.submit("swap_member", { member: props.member.id, ...choice() })}>
    <p>{tr("The member keeps its ID, role, task history, folder and branch. Its old session stays outside the team and is not stopped.")}</p>
    <MemberChoices initial={choice()} onChange={setChoice} disabled={props.busy} />
  </TeamEditDialog>;
}
