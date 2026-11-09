import { For, Show, createSignal, onCleanup, onMount } from "solid-js";
import { AgentGlyph, Icon } from "../lib/icons";
import { claudeApp } from "../../store/claudeApp";
import { machineCards } from "../../store/machines";
import { useNow } from "../../store/latest";
import { native, of } from "./paths";
import { ago, continueTip, stateText, statusText, titleOf, warning } from "./claude-app-ui";
import { tr } from "../../i18n";
import "../styles/claude-app.css";
import { TextInput } from "../lib/TextField";

// WhiteWh fork: Continue a Claude app session (tray/src/ClaudeAppSessionsDialog.h).
// The list, its State column, "This folder only", the Session name and the one
// Continue button are Qt's; Continue sends the resume and the caller closes New
// session. A session the app may still run asks first, with Cancel focused: two
// processes writing one conversation corrupt it.

/** The clock for the Last active column; Qt computes it on each render. */
const useClock = () => useNow(20_000);

export function ClaudeAppDialog(props: {
  /** The New session computer ("" is this one) and its chosen folder. */
  host: string;
  folder: string;
  /** The resume started: close this dialog and New session. */
  onSent(): void;
  onClose(): void;
}) {
  const store = claudeApp;
  let el!: HTMLDialogElement;
  let list!: HTMLUListElement;
  let gone = false;
  const now = useClock();
  const [warningUp, setWarningUp] = createSignal<{ title: string; text: string }>();

  store.openDialog(props.host, props.folder);
  const style = () => of(machineCards[props.host]?.platform, props.host === "", /Win/.test(navigator.platform) ? "windows" : "posix");
  const rows = () => store.visible();
  const row = () => store.selected();
  const folderTip = () => (props.folder ? native(props.folder, style()) : "");
  const status = () => store.busy() ? tr("Reading the Claude app's sessions…") : statusText(store.data(), rows().length, store.onlyFolder(), store.error());
  const tip = () => continueTip(row(), store.problem());

  onMount(() => {
    el.showModal();
    queueMicrotask(() => list.focus());
  });
  onCleanup(() => {
    gone = true;
    store.close();
    if (el.open) el.close();
  });

  const close = () => { if (!gone) props.onClose(); };
  const proceed = () => {
    const outcome = store.begin();
    if (outcome === "asked") {
      const chosen = row();
      if (chosen) setWarningUp(warning(chosen));
      return;
    }
    if (outcome === "sent") props.onSent();
  };
  const answer = (force: boolean) => {
    setWarningUp(undefined);
    if (store.answer(force)) props.onSent();
  };
  const move = (by: number) => {
    const all = rows();
    const at = all.findIndex((item) => item.id === row()?.id);
    const next = all[Math.max(0, Math.min(all.length - 1, at + by))];
    if (next) store.select(next.id);
  };
  const keys = (e: KeyboardEvent) => {
    if (warningUp()) return;
    const target = e.target as HTMLElement;
    if (target.closest(".cl-list") && (e.key === "ArrowDown" || e.key === "ArrowUp")) {
      e.preventDefault();
      move(e.key === "ArrowDown" ? 1 : -1);
      return;
    }
    if (e.key === "Enter" && target.tagName !== "BUTTON" && store.canContinue()) { e.preventDefault(); proceed(); }
  };

  return (
    <dialog class="dlg cl-dlg" ref={el} aria-labelledby="cl-title" onKeyDown={keys}
      onCancel={(e) => { e.preventDefault(); close(); }} onClose={() => { if (!gone) close(); }}>
      <div class="dlg-head"><AgentGlyph agent="claude" size={18} class="accent" /><h2 id="cl-title">{tr("Claude app sessions")}</h2><span class="grow" />
        <button type="button" class="icon-btn" title={tr("Close")} aria-label={tr("Close")} onClick={close}><Icon name="x" size={15} /></button></div>
      <div class="dlg-body cl-body">
        <p class="cl-intro">{tr("Continue a session of the Claude desktop app in Zerus. Zerus starts Claude Code with that conversation in its folder; the app's own copy stays as it is. Stop the session in the app first.")}</p>
        <div class="row cl-tools">
          <label class="check" classList={{ disabled: !props.folder }} title={folderTip()}>
            <input type="checkbox" id="cl-only-folder" checked={store.onlyFolder()} disabled={!props.folder}
              onChange={(e) => store.setOnlyFolder(e.currentTarget.checked)} />{tr("This folder only")}
          </label>
          <span class="grow" />
          <button type="button" class="btn small" disabled={store.busy()} onClick={() => store.refresh()}>{tr("Refresh")}</button>
        </div>
        <ul class="cl-list" ref={list} role="listbox" aria-label={tr("Claude app sessions")} tabindex="0"
          aria-activedescendant={row() ? `cl-row-${row()!.id}` : undefined}>
          <li class="cl-row cl-head" aria-hidden="true">
            <span>{tr("Session")}</span><span>{tr("Folder")}</span><span>{tr("Last active")}</span><span>{tr("State")}</span>
          </li>
          <For each={rows()}>{(item) => (
            <li id={`cl-row-${item.id}`} class="cl-row" role="option" aria-selected={item.id === row()?.id}
              classList={{ on: item.id === row()?.id }}
              title={item.title || item.conversation} onClick={() => store.select(item.id)}
              onDblClick={() => { if (item.id === row()?.id && store.canContinue()) proceed(); }}>
              <span class="ell">{titleOf(item)}</span>
              <span class="ell mono" title={native(item.cwd, style())}>{native(item.cwd, style())}</span>
              <span class="ell">{ago(item.last_activity, now())}</span>
              <span class="ell">{stateText(item.state)}</span>
            </li>
          )}</For>
        </ul>
        <p class="cl-status" classList={{ error: !!store.error() }} title={status()}>{status()}</p>
        <div class="form-row">
          <label for="cl-name">{tr("Session name")}</label>
          <TextInput id="cl-name" class="field mono" value={store.name()} disabled={!row()}
            onInput={(e) => store.setName(e.currentTarget.value)} />
        </div>
        <Show when={store.problem()}><p class="hint dlg-error">{store.problem()}</p></Show>
      </div>
      <div class="dlg-foot">
        <span class="spacer" />
        <button type="button" class="btn" onClick={close}>{tr("Cancel")}</button>
        <button type="button" class="btn primary" disabled={!store.canContinue() || !!warningUp()} title={tip() || undefined} onClick={proceed}>
          {store.saving() ? tr("Continuing…") : tr("Continue in Zerus")}</button>
      </div>
      <Show when={warningUp()} keyed>{(copy) => (
        <RunningWarning copy={copy} onCancel={() => answer(false)} onConfirm={() => answer(true)} />
      )}</Show>
    </dialog>
  );
}

/** Qt's "Still open in the Claude app?" warning: Continue anyway, or Cancel (focused). */
function RunningWarning(props: { copy: { title: string; text: string }; onCancel(): void; onConfirm(): void }) {
  let el!: HTMLDialogElement;
  let gone = false;
  onMount(() => { el.showModal(); queueMicrotask(() => el.querySelector<HTMLElement>("[autofocus]")?.focus()); });
  onCleanup(() => { gone = true; if (el.open) el.close(); });
  return (
    <dialog class="dlg cl-warn" ref={el} role="alertdialog" aria-labelledby="cl-warn-title" aria-describedby="cl-warn-body"
      onCancel={(e) => { e.preventDefault(); props.onCancel(); }} onClose={() => { if (!gone) props.onCancel(); }}>
      <div class="dlg-head"><Icon name="triangle-alert" size={18} class="accent" /><h2 id="cl-warn-title">{props.copy.title}</h2></div>
      <div class="dlg-body"><p class="confirm-body" id="cl-warn-body">{props.copy.text}</p></div>
      <div class="dlg-foot">
        <span class="spacer" />
        <button type="button" class="btn solid-danger" onClick={props.onConfirm}>{tr("Continue anyway")}</button>
        <button type="button" class="btn primary" autofocus onClick={props.onCancel}>{tr("Cancel")}</button>
      </div>
    </dialog>
  );
}
