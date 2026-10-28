import { createStore } from "solid-js/store";
import { anyMachine } from "../../mock/lab-fleet";

// Machine label colors are saved on this device (MachineAppearance): a chosen
// hex color, or the automatic palette entry, in a soft or bright fill.

export const [labels, setLabels] = createStore<Record<string, { hex?: string; vivid: boolean }>>({});

export const labelColor = (alias: string) =>
  labels[alias]?.hex ?? `var(--z-entity-palette-${anyMachine(alias)?.colorIndex ?? 7})`;

/** The colored machine label (IdentityBadges::Machine), honoring this device's color choice. */
export function MachineLabel(props: { alias: string; class?: string }) {
  return (
    <span class={`entity mach-label ${props.class ?? ""}`} classList={{ vivid: labels[props.alias]?.vivid }} style={{ "--entity": labelColor(props.alias) }}>
      {props.alias}
    </span>
  );
}
