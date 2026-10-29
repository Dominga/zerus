import { For, Show, createEffect, createMemo, on, onCleanup, onMount, untrack } from "solid-js";
import { Icon } from "../lib/icons";
import { questions } from "../../store/questions";
import { terminal } from "../../store/terminal";
import { setDetailTab } from "../../store/ui";
import { Body } from "./Activity";
import type { FormView, OptionView } from "../../types";

/** The shown question card (Qt's QuestionCard), between Activity and the composer (SessionsWindow.cpp:918, 1030). */
export function QuestionCard() {
  // Rebuilt for a new card or a rebuilt one only: typing never remounts the fields.
  const ident = createMemo(() => { const q = questions.view(); return q ? `${q.card}\n${q.revision}` : undefined; });
  return (
    <>
      <Show when={ident()} keyed>{(_ident) => <Card />}</Show>
      <ColdAnswerDialog />
    </>
  );
}

/** Rust's answers come back with every grown epoch; `apply` puts them into a field the user may have changed. */
function onEpoch(apply: () => void) {
  createEffect(on(() => questions.view()?.epoch, () => apply(), { defer: true }));
}

function Card() {
  const q = () => questions.view()!;
  const c = () => q().view;
  const formId = () => c().forms[c().page]?.id;
  let el!: HTMLDivElement;
  onMount(() => {
    // The review delay counts only while the card is on screen and the window is active (237-246).
    let visible = false;
    const update = () => questions.setActive(visible && document.visibilityState === "visible" && document.hasFocus());
    const observer = new IntersectionObserver((entries) => { visible = entries.some((e) => e.isIntersecting); update(); });
    observer.observe(el);
    window.addEventListener("focus", update);
    window.addEventListener("blur", update);
    document.addEventListener("visibilitychange", update);
    onCleanup(() => {
      observer.disconnect();
      window.removeEventListener("focus", update);
      window.removeEventListener("blur", update);
      document.removeEventListener("visibilitychange", update);
      questions.setActive(false);
    });
  });
  const native = () => q().terminal_label === "Open native UI";
  const terminalTip = () => (native() ? "Arrives with Native UI." : terminal.texts()?.available ? undefined : terminal.texts()?.reason);
  const onKey = (e: KeyboardEvent) => {
    if (e.key !== "Enter" || e.shiftKey || e.ctrlKey || e.altKey || e.metaKey) return;
    e.preventDefault();
    void questions.enter(e.repeat);
  };
  return (
    <div class="qcard" ref={el} role="group" aria-label={c().heading}>
      <div class="qcard-head">
        <b class="qcard-title">{c().heading}</b><span class="grow" />
        <Show when={c().progress}><span class="qcard-dim">{c().progress}</span></Show>
      </div>
      <div class="qcard-meta">
        <Show when={q().asked}><span class="qcard-dim" title={q().asked_tip}>{q().asked}</span></Show>
        <span class="grow" />
        <Show when={c().pending}><span class="qcard-dim" title={c().pending_tip}>{c().pending}</span></Show>
        <Show when={c().nav}>{(nav) => (
          <>
            <button type="button" class="icon-btn small" aria-label="Previous queued question" title={nav().previous_tip} disabled={!nav().previous}
              onClick={() => void questions.navigate(-1)}><Icon name="chevron-left" size={12} /></button>
            <button type="button" class="icon-btn small" aria-label="Next queued question" title={nav().next_tip} disabled={!nav().next}
              onClick={() => void questions.navigate(1)}><Icon name="chevron-right" size={12} /></button>
          </>
        )}</Show>
      </div>
      <Show when={c().tabs_visible}>
        <div class="qcard-tabs tabs" role="tablist">
          <For each={c().forms}>{(f, i) => (
            <button type="button" class="tab" role="tab" classList={{ on: i() === c().page }} aria-selected={i() === c().page} disabled={c().locked}
              onClick={() => void questions.page(i())}>{f.tab}</button>
          )}</For>
        </div>
      </Show>
      <Show when={formId()} keyed>{(id) => <Form id={id} onKey={onKey} />}</Show>
      <p class="qcard-status" classList={{ error: c().status_error }} title={c().status}>{c().status}</p>
      <div class="qcard-actions">
        <button type="button" class="btn ghost" disabled={native() || !terminal.texts()?.available} title={terminalTip()}
          onClick={() => setDetailTab("terminal")}>{q().terminal_label}</button>
        <span class="grow" />
        <Show when={c().skip.visible}><button type="button" class="btn ghost" title={c().skip_tip} disabled={!questions.enabled(c().skip)} onClick={() => void questions.skip()}>{c().skip.label}</button></Show>
        <Show when={c().retry}><button type="button" class="btn ghost" title={c().retry_tip} onClick={() => void questions.allowRetry()}>Allow retry</button></Show>
        <Show when={c().previous.visible}><button type="button" class="btn ghost" disabled={!c().previous.enabled} onClick={() => void questions.page(c().page - 1)}>{c().previous.label}</button></Show>
        <Show when={c().next.visible}><button type="button" class="btn ghost accent" disabled={!c().next.enabled} onClick={() => void questions.page(c().page + 1)}>{c().next.label}</button></Show>
        <Show when={c().submit.visible}><button type="button" class="btn primary" disabled={!questions.enabled(c().submit)} onClick={() => void questions.submit()}>{questions.label(c().submit)}</button></Show>
        <Show when={c().deny.visible}><button type="button" class="btn ghost" disabled={!questions.enabled(c().deny)} onClick={() => void questions.decide(false)}>{c().deny.label}</button></Show>
        <Show when={c().approve.visible}><button type="button" class="btn primary" disabled={!questions.enabled(c().approve)} onClick={() => void questions.decide(true)}>{questions.label(c().approve)}</button></Show>
      </div>
    </div>
  );
}

function Form(props: { id: string; onKey: (e: KeyboardEvent) => void }) {
  const f = () => questions.view()?.view.forms.find((form) => form.id === props.id);
  const group = `qcard-${props.id}`;
  return (
    <Show when={f()}>{(form) => (
      <div class="qcard-page">
        <div class="qcard-scroll">
          <Show when={form().markdown} fallback={<p class="qcard-prompt wrap-pre">{form().prompt}</p>}><div class="qcard-prompt"><Body text={form().prompt} /></div></Show>
          <Show when={form().body}>
            <Show when={form().markdown} fallback={<p class="qcard-body wrap-pre" classList={{ mono: form().monospace_body }}>{form().body}</p>}><div class="qcard-body"><Body text={form().body} /></div></Show>
          </Show>
          <Show when={form().mode_visible}><p class="qcard-mode">{form().mode}</p></Show>
          <For each={form().options}>{(o) => <Choice form={form()} id={o.id} group={group} onKey={props.onKey} />}</For>
          <Show when={form().other}><Choice form={form()} group={group} other onKey={props.onKey} /></Show>
        </div>
        <Show when={form().text?.visible}><AnswerLine id={props.id} onKey={props.onKey} /></Show>
      </div>
    )}</Show>
  );
}

/** The free-text line: the user's text until Rust hands its own back with a grown epoch. */
function AnswerLine(props: { id: string; onKey: (e: KeyboardEvent) => void }) {
  const t = () => questions.view()?.view.forms.find((form) => form.id === props.id)?.text;
  let input!: HTMLInputElement;
  onEpoch(() => { const value = t()?.value ?? ""; if (input && input.value !== value) input.value = value; });
  return (
    <input ref={input} class="qcard-text" type="text" placeholder={t()?.placeholder} aria-label={t()?.accessible_name} maxLength={t()?.max_units}
      disabled={!t()?.enabled} value={untrack(() => t()?.value ?? "")}
      onInput={(e) => {
        const el = e.currentTarget;
        const max = t()?.max_chars ?? 0;
        // OpenCode counts its bound in characters, as Qt's CharacterLimit does (785-786).
        if (max > 0 && [...el.value].length > max) el.value = [...el.value].slice(0, max).join("");
        questions.edit({ kind: "text", item: props.id, text: el.value });
      }}
      onKeyDown={props.onKey} />
  );
}

function Choice(props: { form: FormView; id?: string; group: string; other?: boolean; onKey: (e: KeyboardEvent) => void }) {
  const o = (): OptionView | undefined => {
    const form = questions.view()?.view.forms.find((f) => f.id === props.form.id);
    return props.other ? form?.other ?? undefined : form?.options.find((option) => option.id === props.id);
  };
  let input: HTMLInputElement | undefined;
  onEpoch(() => { const checked = !!o()?.checked; if (input && input.checked !== checked) input.checked = checked; });
  const toggle = (checked: boolean) => questions.edit(props.other
    ? { kind: "other", item: props.form.id, checked }
    : { kind: "option", item: props.form.id, option: props.id ?? "", checked });
  return (
    <Show when={o()?.visible}>
      <label class="qchoice" classList={{ on: !!o()?.checked, off: !o()?.enabled }}>
        <input ref={input} type={props.form.multi ? "checkbox" : "radio"} name={props.group} checked={!!o()?.checked} disabled={!o()?.enabled}
          onChange={(e) => toggle(e.currentTarget.checked)} onKeyDown={props.onKey} />
        <span class="qchoice-copy">
          <span class="qchoice-title">{o()?.label}</span>
          <Show when={o()?.description}>
            <Show when={o()?.markdown} fallback={<span class="qchoice-desc wrap-pre">{o()?.description}</span>}><span class="qchoice-desc"><Body text={o()?.description ?? ""} /></span></Show>
          </Show>
        </span>
      </label>
    </Show>
  );
}

/** "Submit with a cold cache?": Cancel is the default and Escape (SessionsWindow.cpp:5111-5131). */
function ColdAnswerDialog() {
  let el: HTMLDialogElement | undefined;
  createEffect(() => {
    const d = questions.dialog();
    if (!el) return;
    if (!d) { if (el.open) el.close(); return; }
    if (!el.open) el.showModal();
    queueMicrotask(() => el?.querySelector<HTMLElement>("[data-initial]")?.focus());
  });
  return (
    <dialog class="dlg" ref={el} role="alertdialog" aria-labelledby="qcard-cold-title" aria-describedby="qcard-cold-text" style={{ width: "460px" }}
      onClose={() => { if (questions.dialog()) void questions.answerCold(false); }}>
      <Show when={questions.dialog()}>{(d) => (
        <>
          <div class="dlg-head"><Icon name="triangle-alert" size={18} class="accent" /><h2 id="qcard-cold-title">{d().prompt.title}</h2></div>
          <div class="dlg-body"><div class="confirm-body wrap-pre" id="qcard-cold-text">{d().prompt.text}</div></div>
          <div class="dlg-foot">
            <span class="spacer" />
            <button type="button" class="btn primary" data-initial autofocus onClick={() => void questions.answerCold(false)}>Cancel</button>
            <button type="button" class="btn" onClick={() => void questions.answerCold(true)}>{d().prompt.proceed}</button>
          </div>
        </>
      )}</Show>
    </dialog>
  );
}
