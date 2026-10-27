import { For, Show, createEffect, createRoot, createSignal, type JSX } from "solid-js";
import { Icon } from "../../lib/icons";
import { dialog, setDialog } from "../../state";
import { projectColor, type EditableProject } from "./store";
import "../../styles/setup.css";

// ---------------------------------------------------------------------------
// Stacked dialogs. The shared `dialog` signal holds one id; setup flows open
// child dialogs over their parent (New session -> New worktree, Clone ->
// Choose folder -> New folder), so children live on a local stack. Native
// <dialog>.showModal() keeps the newest one on top of the top layer.

const [stack, setStack] = createSignal<string[]>([]);
createRoot(() => createEffect(() => { if (!dialog()) setStack([]); }));

export const isOpen = (id: string) => dialog() === id || stack().includes(id);

/** Open a dialog: over the current one when another dialog is showing. */
export function openSetup(id: string) {
  if (isOpen(id)) return;
  if (dialog()) setStack((s) => [...s, id]);
  else setDialog(id);
}

export function closeSetup(id: string) {
  if (stack().includes(id)) setStack((s) => s.filter((x) => x !== id));
  else if (dialog() === id) setDialog(undefined);
}

/** Replace one step of a flow with the next (Clone on computer -> Clone repository). */
export function replaceSetup(from: string, to: string) {
  if (dialog() === from) setDialog(to);
  else setStack((s) => s.map((x) => (x === from ? to : x)));
}

export function SDialog(props: {
  id: string;
  title: string;
  width?: string;
  height?: string;
  icon?: string;
  class?: string;
  /** While busy, Escape, the close button and Cancel are blocked. */
  busy?: () => boolean;
  /** An outside click cancels (popups such as the project color). */
  lightDismiss?: boolean;
  head?: JSX.Element;
  footer?: JSX.Element;
  /** The child renders its own .dlg-body and .dlg-foot (shared form state). */
  bare?: boolean;
  children: JSX.Element;
}) {
  let el!: HTMLDialogElement;
  const open = () => isOpen(props.id);
  createEffect(() => {
    if (open()) {
      if (el.open) return;
      el.showModal();
      // Without an explicit [autofocus], start in the form rather than on the close button.
      if (!el.querySelector("[autofocus]")) {
        el.querySelector<HTMLElement>(".dlg-body :is(input, select, textarea, [tabindex='0']):not(:disabled):not([type=hidden])")?.focus({ preventScroll: true });
      }
    } else if (el.open) el.close();
  });
  return (
    <dialog class={`dlg su-dlg ${props.class ?? ""}`} ref={el} aria-labelledby={`${props.id}-title`} closedby={props.lightDismiss ? "any" : undefined}
      style={{ width: props.width, height: props.height }}
      onCancel={(e) => { if (props.busy?.()) e.preventDefault(); }}
      onClose={() => { if (open()) closeSetup(props.id); }}
      onClick={(e) => {
        // Fallback light dismiss where `closedby` is unsupported.
        if (!props.lightDismiss || e.target !== el) return;
        const r = el.getBoundingClientRect();
        if (e.clientX < r.left || e.clientX > r.right || e.clientY < r.top || e.clientY > r.bottom) closeSetup(props.id);
      }}>
      <Show when={open()}>
        <div class="dlg-head">
          <Show when={props.icon}><Icon name={props.icon!} size={18} class="accent" /></Show>
          <h2 id={`${props.id}-title`} class="ell">{props.title}</h2>
          {props.head}
          <span class="grow" />
          <button class="icon-btn" aria-label="Close" disabled={props.busy?.()} onClick={() => closeSetup(props.id)}><Icon name="x" size={16} /></button>
        </div>
        <Show when={!props.bare} fallback={props.children}>
          <div class="dlg-body">{props.children}</div>
          <Show when={props.footer}><div class="dlg-foot">{props.footer}</div></Show>
        </Show>
      </Show>
    </dialog>
  );
}

// ---------------------------------------------------------------------------
// Rich select: a native <select> opted into the customizable state, so the
// picker can show color swatches and badges. Browsers without it fall back to
// the plain list with the same readable option text.

export interface RichOption { value: string; label: string; swatch?: string; vivid?: boolean; detail?: string; icon?: string; disabled?: boolean }

/** Progressive enhancement: the button and <selectedcontent> exist only where supported. */
const richSelects = typeof CSS !== "undefined" && CSS.supports("appearance", "base-select");
function enhance(el: HTMLSelectElement) {
  if (!richSelects) return;
  const button = document.createElement("button");
  button.type = "button";
  button.append(document.createElement("selectedcontent"));
  el.prepend(button);
}

export function RichSelect(props: {
  value: string;
  options: RichOption[];
  onChange: (value: string) => void;
  label: string;
  class?: string;
  disabled?: boolean;
  placeholder?: string;
  id?: string;
}) {
  return (
    <select class={`field su-rsel ${props.class ?? ""}`} id={props.id} aria-label={props.label} disabled={props.disabled} ref={enhance}
      value={props.value} onChange={(e) => props.onChange(e.currentTarget.value)}>
      <Show when={props.placeholder}><option value="" disabled>{props.placeholder}</option></Show>
      <For each={props.options}>
        {(o) => (
          <option value={o.value} disabled={o.disabled} aria-label={o.detail ? `${o.label} ${o.detail}` : o.label} selected={o.value === props.value}>
            <Show when={o.swatch}><span class="su-rssw entity" classList={{ vivid: o.vivid }} style={{ "--entity": o.swatch }}><Icon name="folder" size={11} /></span></Show>
            <Show when={o.icon}><Icon name={o.icon!} size={13} class="su-rsico" /></Show>
            <span class="su-rslabel">{o.label}</span>
            <Show when={o.detail}><span class="su-rsdetail">{o.detail}</span></Show>
          </option>
        )}
      </For>
    </select>
  );
}

// ---------------------------------------------------------------------------

export function Segmented<T extends string>(props: {
  value: T;
  options: { value: T; label: string; icon?: string; tip?: string; disabled?: boolean }[];
  onChange: (v: T) => void;
  label: string;
  disabled?: boolean;
}) {
  return (
    <div class="su-seg" role="group" aria-label={props.label}>
      <For each={props.options}>
        {(o) => (
          <button type="button" class="su-segbtn" classList={{ on: props.value === o.value }} aria-pressed={props.value === o.value}
            disabled={props.disabled || o.disabled} data-tip={o.tip} onClick={() => props.onChange(o.value)}>
            <Show when={o.icon}><Icon name={o.icon!} size={14} /></Show>
            {o.label}
          </button>
        )}
      </For>
    </div>
  );
}

/** Project label painting (ProjectAppearance.h): rounded fill, icon, name, Default badge. */
export function ProjectLabel(props: { project: Pick<EditableProject, "name" | "color" | "colorIndex" | "vivid" | "isDefault" | "id">; class?: string; small?: boolean }) {
  return (
    <span class={`su-plabel entity ${props.class ?? ""}`} classList={{ vivid: props.project.vivid, small: props.small }} style={{ "--entity": projectColor(props.project) }}>
      <Icon name={props.project.id === "general" ? "layers" : "folder"} size={props.small ? 12 : 14} stroke={1.8} />
      <span class="ell">{props.project.name}</span>
      <Show when={props.project.isDefault}><span class="su-pldefault">Default</span></Show>
    </span>
  );
}

/** Review-only control that flips simulated outcomes. Not part of the product UI. */
export function MockSwitch<T extends string>(props: { label: string; value: T; options: { value: T; label: string }[]; onChange: (v: T) => void }) {
  return (
    <label class="su-mock" title="Mockup only: choose the simulated result">
      <span class="tag">MOCKUP</span>
      <span>{props.label}</span>
      <select value={props.value} onChange={(e) => props.onChange(e.currentTarget.value as T)}>
        <For each={props.options}>{(o) => <option value={o.value}>{o.label}</option>}</For>
      </select>
    </label>
  );
}

export function Progress(props: { value?: number; label?: string }) {
  return (
    <div class="su-progress" classList={{ "su-indet": props.value === undefined }} role="progressbar" aria-label={props.label}
      aria-valuenow={props.value} aria-valuemin={0} aria-valuemax={100}>
      <i style={props.value !== undefined ? { width: `${props.value}%` } : undefined} />
    </div>
  );
}

/** Paths are shown in the computer's own style. */
export const leaf = (path: string) => path.split(/[\\/]/).filter(Boolean).pop() ?? path;
export const parentOf = (path: string) => {
  const sep = path.includes("\\") ? "\\" : "/";
  const parts = path.split(sep);
  parts.pop();
  return parts.join(sep) || sep;
};
export const join = (parent: string, name: string) => {
  const sep = parent.includes("\\") ? "\\" : "/";
  return parent.endsWith(sep) ? parent + name : parent + sep + name;
};
export const hex8 = () => Array.from(crypto.getRandomValues(new Uint8Array(4)), (b) => b.toString(16).padStart(2, "0")).join("");
