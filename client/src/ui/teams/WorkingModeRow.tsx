import type { JSX } from "solid-js";
import { tr } from "../../i18n";
import { Icon } from "../lib/icons";

/**
 * Task #337: the project's working-mode suggestion as one compact row, as Qt's
 * team bar chip is. The reasons stay in the row's tooltip; the owner can keep
 * the team's settings (the block goes away until the profile changes), propose
 * the switch, or close it for this session of the profile.
 */
export function WorkingModeRow(props: {
  preset: string;
  title: string;
  reasons: readonly string[];
  disagreeing: readonly string[];
  onKeep(): void;
  onPropose(): void;
  onClose(): void;
}): JSX.Element {
  const detail = () => [props.title, ...props.reasons].filter(Boolean).join("\n");
  return (
    <section class="team-working-mode" data-working-mode={props.preset} aria-label={tr("Working mode")} title={detail()}>
      <Icon name="git-branch" size={14} />
      <b>{tr("Working mode: the project suggests %1", props.title || props.preset)}</b>
      <span class="team-working-mode-wrong">{tr("This team's settings differ: %1", props.disagreeing.join(", "))}</span>
      <span class="grow" />
      <button type="button" class="btn small" data-working-mode-keep onClick={() => props.onKeep()}>{tr("Keep this team's settings")}</button>
      <button type="button" class="btn small ghost" data-working-mode-switch onClick={() => props.onPropose()}>{tr("Propose the switch")}</button>
      <button type="button" class="icon-btn small" data-working-mode-close aria-label={tr("Close")}
        title={tr("Hide this until the project's profile changes")} onClick={() => props.onClose()}><Icon name="x" size={13} /></button>
    </section>
  );
}
