import { Show } from "solid-js";
import { Icon } from "../lib/icons";
import { settings } from "../../store/settings";
import { projectColor } from "../screens/projects-ui";
import type { Project } from "../../types";
/** The mockup's project fill; vivid stays local and keyed by the exact ID. */
export function ProjectLabel(props: { project: Project; class?: string; vivid?: boolean }) {
  return <span class={`su-plabel entity ${props.class ?? ""}`} classList={{ vivid: props.vivid ?? settings[`projects/vivid/${props.project.id}`] === true }} style={{ "--entity": projectColor(props.project) }}>
    <Icon name={props.project.id === "ungrouped" ? "layers" : "folder"} size={14} stroke={1.8} />
    <span class="ell">{props.project.name}</span><Show when={props.project.is_default}><span class="su-pldefault">Default</span></Show>
  </span>;
}
