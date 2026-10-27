import { For, Show, type JSX } from "solid-js";
import { Icon } from "../../lib/icons";
import "../../styles/settings.css";

// Small building blocks shared by the Settings sections, Machines and
// Accounts. They mirror the Qt widgets each page uses (QCheckBox, QComboBox,
// QSlider + reset button, QDoubleSpinBox, settingsCard frames) without
// adding behavior of their own.

export function SectionHead(props: { title: string; intro?: JSX.Element; actions?: JSX.Element }) {
  return (
    <header class="set-head">
      <div class="row">
        <h2>{props.title}</h2>
        <span class="grow" />
        {props.actions}
      </div>
      <Show when={props.intro}><p class="set-intro">{props.intro}</p></Show>
    </header>
  );
}

export function Group(props: { title?: string; hint?: JSX.Element; aside?: JSX.Element; children: JSX.Element; class?: string; flat?: boolean }) {
  return (
    <section class={`set-group ${props.class ?? ""}`} classList={{ flat: props.flat }}>
      <Show when={props.title || props.aside}>
        <div class="set-group-head">
          <Show when={props.title}><h3>{props.title}</h3></Show>
          <span class="grow" />
          {props.aside}
        </div>
      </Show>
      <Show when={props.hint}><p class="set-group-hint">{props.hint}</p></Show>
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

export function Check(props: { label: JSX.Element; checked: boolean; onChange: (on: boolean) => void; disabled?: boolean; hint?: JSX.Element }) {
  return (
    <div class="set-check" classList={{ disabled: props.disabled }}>
      <label class="check">
        <input type="checkbox" checked={props.checked} disabled={props.disabled} onChange={(e) => props.onChange(e.currentTarget.checked)} />
        <span>{props.label}</span>
      </label>
      <Show when={props.hint}><div class="set-hint">{props.hint}</div></Show>
    </div>
  );
}

export interface Option { value: string; label: string; disabled?: boolean }

export function Select(props: { value: string; options: Option[]; onChange: (value: string) => void; disabled?: boolean; label?: string; id?: string; class?: string }) {
  return (
    <select id={props.id} class={`field set-select ${props.class ?? ""}`} value={props.value} disabled={props.disabled} aria-label={props.label}
      onChange={(e) => props.onChange(e.currentTarget.value)}>
      <For each={props.options}>{(o) => <option value={o.value} disabled={o.disabled} selected={o.value === props.value}>{o.label}</option>}</For>
    </select>
  );
}

export function NumberField(props: { value: number; min: number; max: number; step?: number; suffix?: string; onChange: (v: number) => void; disabled?: boolean; id?: string; label?: string; decimals?: number }) {
  const clamp = (v: number) => Math.min(props.max, Math.max(props.min, v));
  return (
    <span class="set-number" classList={{ disabled: props.disabled }}>
      <input id={props.id} class="field tabular" type="number" min={props.min} max={props.max} step={props.step ?? 1} value={props.value.toFixed(props.decimals ?? 0)}
        disabled={props.disabled} aria-label={props.label} onChange={(e) => props.onChange(clamp(Number(e.currentTarget.value) || props.min))} />
      <Show when={props.suffix}><span class="set-suffix">{props.suffix}</span></Show>
    </span>
  );
}

/** QSlider with its value and a reset button that keeps its space while hidden (SettingsPage.h:200-215). */
export function Slider(props: { value: number; min: number; max: number; step: number; defaultValue: number; format: (v: number) => string; onChange: (v: number) => void; disabled?: boolean; label: string; resetLabel: string }) {
  let input!: HTMLInputElement;
  return (
    <div class="set-slider" classList={{ disabled: props.disabled }}>
      <input ref={input} type="range" min={props.min} max={props.max} step={props.step} value={props.value} disabled={props.disabled} aria-label={props.label}
        style={{ "--fill": `${((props.value - props.min) / (props.max - props.min)) * 100}%` }}
        onInput={(e) => props.onChange(Number(e.currentTarget.value))} />
      <span class="set-slider-value tabular">{props.format(props.value)}</span>
      <button class="icon-btn small set-reset" classList={{ hidden: props.disabled || props.value === props.defaultValue }} title={props.resetLabel} aria-label={props.resetLabel}
        onClick={() => { input.focus(); props.onChange(props.defaultValue); }}>
        <Icon name="rotate-ccw" size={13} />
      </button>
    </div>
  );
}

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

/** Simple data table with a selectable row and fixed columns. */
export function Table<T>(props: {
  columns: { key: string; label: string; class?: string; render: (row: T) => JSX.Element }[];
  rows: T[];
  rowKey: (row: T) => string;
  selected?: string;
  onSelect?: (key: string) => void;
  onActivate?: (key: string, column: number) => void;
  label: string;
  empty?: JSX.Element;
  class?: string;
}) {
  return (
    <div class={`set-table-wrap ${props.class ?? ""}`}>
      <table class="set-table" aria-label={props.label}>
        <thead>
          <tr><For each={props.columns}>{(c) => <th class={c.class} scope="col">{c.label}</th>}</For></tr>
        </thead>
        <tbody>
          <For each={props.rows} fallback={<tr class="set-table-empty"><td colspan={props.columns.length}>{props.empty}</td></tr>}>
            {(row) => {
              const key = props.rowKey(row);
              return (
                <tr classList={{ on: props.selected === key }} tabindex={props.onSelect ? 0 : undefined} aria-selected={props.onSelect ? props.selected === key : undefined}
                  onClick={() => props.onSelect?.(key)}
                  onKeyDown={(e) => { if (e.key === "Enter") props.onActivate?.(key, 1); }}>
                  <For each={props.columns}>{(c, i) => <td class={c.class} onDblClick={() => props.onActivate?.(key, i())}>{c.render(row)}</td>}</For>
                </tr>
              );
            }}
          </For>
        </tbody>
      </table>
    </div>
  );
}
