import { teamHelpLabel, openTeamHelp } from "../../store/teamHelp";
import { Icon } from "../lib/icons";

export function TeamHelpButton(props: { titlebar?: boolean }) {
  return <button type="button" class={props.titlebar ? "tb-btn" : "icon-btn small"} data-team-help
    title={teamHelpLabel()} aria-label={teamHelpLabel()} aria-haspopup="dialog" onClick={openTeamHelp}>
    <Icon name="circle-question-mark" size={17} />
  </button>;
}
