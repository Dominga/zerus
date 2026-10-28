import { For, Show, type JSX } from "solid-js";
import { Icon } from "../lib/icons";

// Status rows and action groups shared by Machines and Accounts (design/webui
// screens/settings/ui.tsx). Settings (3b) uses them too.

export function Status(props: { kind?: "ok" | "warn" | "error" | "busy" | "muted"; children: JSX.Element; class?: string }) {
  return (
    <div class={`set-status k-${props.kind ?? "muted"} ${props.class ?? ""}`} role="status">
      <Show when={props.kind === "busy"}><span class="set-spinner" aria-hidden="true" /></Show>
      <Show when={props.kind === "ok"}><Icon name="check" size={13} /></Show>
      <Show when={props.kind === "warn"}><Icon name="triangle-alert" size={13} /></Show>
      <Show when={props.kind === "error"}><Icon name="circle-alert" size={13} /></Show>
      <span class="set-status-text">{props.children}</span>
    </div>
  );
}

export function Spinner(props: { size?: number }) {
  return <span class="set-spinner" style={{ width: `${props.size ?? 14}px`, height: `${props.size ?? 14}px` }} aria-hidden="true" />;
}

/** A multi-line plain-text value, as Qt labels with \n show it. */
export function Lines(props: { text: string; class?: string }) {
  return <div class={`set-lines ${props.class ?? ""}`}><For each={props.text.split("\n")}>{(line) => <div>{line}</div>}</For></div>;
}

export function Actions(props: { children: JSX.Element; class?: string }) {
  return <div class={`set-actions ${props.class ?? ""}`}>{props.children}</div>;
}
