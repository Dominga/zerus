import { paletteIndex } from "../sessions/SessionCard";

// The coloured machine label (IdentityBadges::Machine). Label colours and
// styles are this device's preferences and arrive with machine editing
// (3c); until then every machine shows its default palette slot.
export function MachineLabel(props: { alias: string; class?: string; dimmed?: boolean }) {
  return (
    <span class={`entity mach-label ${props.class ?? ""}`} classList={{ dimmed: props.dimmed }} style={{ "--entity": `var(--z-entity-palette-${paletteIndex(props.alias)})` }}>
      {props.alias}
    </span>
  );
}
