import { For, Show, type JSX } from "solid-js";
import { Icon } from "../../lib/icons";

// The rows Settings sections are built from, from the approved mockup
// (design/webui/src/screens/settings/ui.tsx). They stand in for the Qt widgets
// each page uses (QCheckBox, QComboBox, QSlider with its reset button)
// without adding behavior of their own.

export function SectionHead(props: { title: string; intro?: JSX.Element }) {
  return (
    <header class="set-head">
      <h2>{props.title}</h2>
      <Show when={props.intro}><p class="set-intro">{props.intro}</p></Show>
    </header>
  );
}

export function Group(props: { title?: string; children: JSX.Element }) {
  return (
    <section class="set-group">
      <Show when={props.title}><div class="set-group-head"><h3>{props.title}</h3></div></Show>
      {props.children}
    </section>
  );
}

/** A labelled setting: text on the left, the control on the right, a hint below. */
export function Row(props: { label: JSX.Element; hint?: JSX.Element; children?: JSX.Element; for?: string; disabled?: boolean; wide?: boolean; stack?: boolean }) {
  return (
    <div class="set-row" classList={{ disabled: props.disabled, wide: props.wide, stack: props.stack }}>
      <div class="set-row-text">
        <label class="set-label" for={props.for}>{props.label}</label>
        <Show when={props.hint}><div class="set-hint">{props.hint}</div></Show>
      </div>
      <Show when={props.children}><div class="set-row-control">{props.children}</div></Show>
    </div>
  );
}

export function Switch(props: { checked: boolean; onChange: (on: boolean) => void; label: string; disabled?: boolean; id?: string }) {
  return (
    <button id={props.id} type="button" role="switch" class="set-switch switch" classList={{ on: props.checked }} aria-checked={props.checked}
      aria-label={props.label} disabled={props.disabled} onClick={() => props.onChange(!props.checked)} />
  );
}

export function ToggleRow(props: { label: string; hint?: JSX.Element; checked: boolean; onChange: (on: boolean) => void; disabled?: boolean }) {
  const id = `t-${props.label.replace(/\W+/g, "-").toLowerCase()}`;
  return (
    <Row label={props.label} hint={props.hint} for={id} disabled={props.disabled}>
      <Switch id={id} checked={props.checked} onChange={props.onChange} label={props.label} disabled={props.disabled} />
    </Row>
  );
}

export function Check(props: { label: JSX.Element; checked: boolean; onChange: (on: boolean) => void; disabled?: boolean }) {
  return (
    <div class="set-check" classList={{ disabled: props.disabled }}>
      <label class="check">
        <input type="checkbox" checked={props.checked} disabled={props.disabled} onChange={(e) => props.onChange(e.currentTarget.checked)} />
        <span>{props.label}</span>
      </label>
    </div>
  );
}

export interface Option { value: string; label: string }

export function Select(props: { value: string; options: Option[]; onChange: (value: string) => void; label?: string; id?: string }) {
  return (
    <select id={props.id} class="field set-select" value={props.value} aria-label={props.label}
      onChange={(e) => props.onChange(e.currentTarget.value)}>
      <For each={props.options}>{(o) => <option value={o.value} selected={o.value === props.value}>{o.label}</option>}</For>
    </select>
  );
}

/**
 * A slider with its value and a reset button that appears only after a
 * change and keeps its space while hidden (SettingsPage.h resetButton).
 * `onInput` follows the drag; `onCommit` stores the value once it settles.
 */
export function Slider(props: {
  value: number; min: number; max: number; step: number; defaultValue: number; format: (v: number) => string;
  onInput: (v: number) => void; onCommit: (v: number) => void; disabled?: boolean; label: string; resetLabel: string;
}) {
  let input!: HTMLInputElement;
  return (
    <div class="set-slider" classList={{ disabled: props.disabled }}>
      <input ref={input} type="range" min={props.min} max={props.max} step={props.step} value={props.value} disabled={props.disabled} aria-label={props.label}
        style={{ "--fill": `${((props.value - props.min) / (props.max - props.min)) * 100}%` }}
        onInput={(e) => props.onInput(Number(e.currentTarget.value))}
        onChange={(e) => props.onCommit(Number(e.currentTarget.value))} />
      <span class="set-slider-value tabular">{props.format(props.value)}</span>
      <button class="icon-btn small set-reset" classList={{ hidden: props.disabled || props.value === props.defaultValue }} title={props.resetLabel} aria-label={props.resetLabel}
        onClick={() => { input.focus(); props.onCommit(props.defaultValue); }}>
        <Icon name="rotate-ccw" size={13} />
      </button>
    </div>
  );
}
