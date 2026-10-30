import { paletteIndex } from "../sessions/SessionCard";
import { settings } from "../../store/settings";
import { machineAppearance } from "../../store/machineAppearance";

// The coloured machine label (IdentityBadges::Machine), including local
// preferences imported read-only from Qt by G3.
export function MachineLabel(props: { alias: string; class?: string; dimmed?: boolean }) {
  const appearance = () => machineAppearance(settings, props.alias, `var(--z-entity-palette-${paletteIndex(props.alias)})`);
  return (
    <span class={`entity mach-label ${props.class ?? ""}`} classList={{ dimmed: props.dimmed, vivid: appearance().vivid }} style={{ "--entity": appearance().color }}>
      {props.alias}
    </span>
  );
}
