import { For, Show, createEffect, onCleanup, onMount } from "solid-js";
import { sessionModels as models } from "../../store/sessionModels";
import { terminal } from "../../store/terminal";
import { setDetailTab } from "../../store/ui";
import { ModelList } from "./ModelPicker";
import { Icon } from "../lib/icons";
import "../styles/newsession.css";

function SettingsPopover(props: { anchor: () => HTMLButtonElement | undefined }) {
  let dialog!: HTMLDialogElement;
  let mounted = false;
  const v = () => models.view()!;
  const native = () => v().terminal && !v().enabled;
  const place = () => {
    const anchor = props.anchor()?.getBoundingClientRect(); if (!anchor) return;
    const width = Math.min(v().openCode ? 420 : 360, window.innerWidth - 24);
    dialog.style.width = `${width}px`;
    const height = dialog.getBoundingClientRect().height;
    dialog.style.left = `${Math.max(8, Math.min(anchor.right - width, window.innerWidth - width - 8))}px`;
    dialog.style.top = `${Math.max(8, Math.min(anchor.top - height - 7, window.innerHeight - height - 8))}px`;
  };
  onMount(() => {
    mounted = true; dialog.showModal(); place();
    (dialog.querySelector<HTMLElement>(native() ? ".cmp-settings-terminal" : "input,select") ?? dialog.querySelector<HTMLElement>("button"))?.focus();
    window.addEventListener("resize", place);
  });
  onCleanup(() => {
    mounted = false; window.removeEventListener("resize", place);
    if (dialog.open) dialog.close();
    queueMicrotask(() => props.anchor()?.focus());
  });
  createEffect(() => { models.catalogError(); models.model(); v().hint; if (mounted) queueMicrotask(place); });
  const outside = (e: PointerEvent) => {
    if (e.target !== dialog) return;
    const box = dialog.getBoundingClientRect();
    if (e.clientX < box.left || e.clientX > box.right || e.clientY < box.top || e.clientY > box.bottom) models.close();
  };
  const choices = () => {
    const all = models.options();
    return all.some((m) => m.id === models.model()) || !models.model() ? all
      : [{ id: models.model(), label: models.model(), effort_options: [], effort_labels: [] }, ...all];
  };
  const effortEnabled = () => v().enabled && models.options().some((m) => m.id === models.model())
    && (v().openCode ? models.efforts().length > 1 : !!models.effort());
  return (
    <dialog ref={dialog} class="cmp-settings-pop" aria-label="Model and reasoning effort" onPointerDown={outside}
      onCancel={(e) => { e.preventDefault(); models.close(); }}>
      <Show when={!native()}>
        <label for="session-model-choice">Model</label>
        <Show when={v().openCode} fallback={
          <select id="session-model-choice" class="field" aria-label="Model" value={models.model()} disabled={!v().enabled || choices().length < 2}
            onChange={(e) => models.chooseModel(e.currentTarget.value)}>
            <For each={choices()}>{(m) => <option value={m.id} selected={m.id === models.model()}>{m.label}</option>}</For>
          </select>
        }>
          <fieldset disabled={!v().enabled} class="cmp-settings-models" aria-label="Model">
            <ModelList view={models.picker} act={models.pickerActions} disabled={!v().enabled} />
          </fieldset>
        </Show>
        <label for="session-effort-choice">Reasoning effort</label>
        <select id="session-effort-choice" class="field" aria-label="Reasoning effort" value={models.effort()} disabled={!effortEnabled()}
          onChange={(e) => models.chooseEffort(e.currentTarget.value)}>
          <For each={models.efforts()}>{(e) => <option value={e.value} selected={e.value === models.effort()}>{e.label}</option>}</For>
        </select>
      </Show>
      <p class="cmp-settings-hint" role="status">{v().hint}<Show when={models.catalogError()}>{`\n${models.catalogError()}`}</Show></p>
      <div class="cmp-settings-actions">
        <Show when={native()}><button type="button" class="btn cmp-settings-terminal" onClick={() => { models.close(); terminal.opened(); setDetailTab("terminal"); }}>Open Terminal</button></Show>
        <span class="grow" />
        <button type="button" class="btn" title="Close without applying changes" onClick={() => models.close()}>Cancel</button>
        <Show when={!native()}><button type="button" class="btn primary" disabled={!models.canApply()} onClick={() => { void models.apply(); }}>{v().applyLabel}</button></Show>
      </div>
    </dialog>
  );
}

export function ModelSettingsButton() {
  let anchor: HTMLButtonElement | undefined;
  return <Show when={models.view()}>{(v) => <>
    <button ref={anchor} type="button" class="btn ghost cmp-model" aria-label={v().accessible} title={v().tooltip}
      aria-haspopup="dialog" aria-expanded={models.opened()} onClick={() => models.open()}>
      <span class="ell cmp-model-name">{v().shownModel}</span><span class="dim cmp-model-suffix">{v().suffix}</span><Icon name="chevron-down" size={12} />
    </button>
    <Show when={models.opened()}><SettingsPopover anchor={() => anchor} /></Show>
  </>}</Show>;
}
