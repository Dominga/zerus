import { TextInput } from "../lib/TextField";
import { For, Show, createEffect, createMemo, createSignal } from "solid-js";
import { Icon } from "../lib/icons";
import { chooseParent, detachParent, parentCandidates, parentChangeBusy, parentDialog, setParentDialog } from "../../store/parentLinks";
import type { Session } from "./types";
import { tr } from "../../i18n";

// Detach, Set parent… and the drag-to-nest confirmation (zerus-sa9qn.9). Every
// change of a session's parent link is confirmed here; the host checks it.

const close = () => setParentDialog(undefined);
const short = (name: string) => name.split("/").pop() || name;

function DetachDialog(props: { child: Session }) {
  const parent = () => props.child.parent?.name ?? "";
  const run = async () => { if (await detachParent(props.child)) close(); };
  return (
    <>
      <div class="dlg-head"><Icon name="git-branch" size={18} class="accent" /><h2 id="parent-dlg-title">{tr("Detach from %1?", short(parent()))}</h2></div>
      <div class="dlg-body" id="parent-dlg-body">
        <p>{tr("%1 stops being a child of %2. Its requests draw your attention again, and it moves to the top level of its project.", props.child.title, parent())}</p>
        <p class="dim">{tr("Nothing else changes. The parent session can adopt it again.")}</p>
      </div>
      <div class="dlg-foot"><span class="spacer" />
        <button type="button" class="btn" data-initial disabled={parentChangeBusy()} onClick={close}>{tr("Cancel")}</button>
        <button type="button" class="btn primary" disabled={parentChangeBusy()} onClick={() => void run()}>{tr("Detach")}</button>
      </div>
    </>
  );
}

function SetDialog(props: { child: Session; parent?: Session }) {
  const [query, setQuery] = createSignal("");
  const [chosen, setChosen] = createSignal<Session | undefined>(props.parent);
  const all = createMemo(() => parentCandidates(props.child));
  const shown = createMemo(() => {
    const q = query().trim().toLowerCase();
    return all().filter((s) => !q || s.name.toLowerCase().includes(q) || s.title.toLowerCase().includes(q)).slice(0, 200);
  });
  const current = () => props.child.parent?.name;
  const run = async () => { const parent = chosen(); if (parent && await chooseParent(props.child, parent)) close(); };
  return (
    <form onSubmit={(e) => { e.preventDefault(); void run(); }}>
      <div class="dlg-head"><Icon name="git-branch" size={18} class="accent" /><h2 id="parent-dlg-title">
        {props.parent ? tr("Make %1 a child of %2?", short(props.child.title), short(props.parent.title)) : tr("Parent session for %1", short(props.child.title))}
      </h2></div>
      <div class="dlg-body col" id="parent-dlg-body" style={{ gap: "6px" }}>
        <Show when={!props.parent}>
          <TextInput class="dlg-input" data-initial type="search" placeholder={tr("Search sessions on this computer")} value={query()}
            aria-label={tr("Search sessions on this computer")} onInput={(e) => setQuery(e.currentTarget.value)} />
          <div class="parent-choices" role="listbox" aria-label={tr("Sessions")}>
            <For each={shown()} fallback={<p class="dim">{tr("No other session on this computer can be its parent.")}</p>}>{(s) => (
              <button type="button" class="menu-item" role="option" aria-selected={chosen()?.key === s.key} classList={{ on: chosen()?.key === s.key }}
                onClick={() => setChosen(s)}>
                <span class="ell">{s.title}</span><span class="grow" /><span class="dim ell">{s.name}</span>
              </button>
            )}</For>
          </div>
        </Show>
        <p>{tr("Its replies, questions and errors go quietly to the parent while the parent runs. Approvals and sign-in still draw your attention.")}</p>
        <Show when={current()}>{(old) => <p class="warn-text">{tr("It is now a child of %1; this replaces that parent.", old())}</p>}</Show>
      </div>
      <div class="dlg-foot"><span class="spacer" />
        <button type="button" class="btn" data-initial={props.parent ? true : undefined} disabled={parentChangeBusy()} onClick={close}>{tr("Cancel")}</button>
        <button type="submit" class="btn primary" disabled={parentChangeBusy() || !chosen()}>{props.parent ? tr("Make child") : tr("Set parent")}</button>
      </div>
    </form>
  );
}

export function ParentDialogs() {
  let el: HTMLDialogElement | undefined;
  createEffect(() => {
    const state = parentDialog();
    if (!el) return;
    if (!state) { if (el.open) el.close(); return; }
    if (!el.open) el.showModal();
    queueMicrotask(() => el?.querySelector<HTMLElement>("[data-initial]")?.focus());
  });
  return (
    <dialog class="dlg" ref={el} role="alertdialog" aria-labelledby="parent-dlg-title" aria-describedby="parent-dlg-body" style={{ width: "460px" }} onClose={close}>
      <Show when={parentDialog()} keyed>
        {(d) => d.kind === "detach" ? <DetachDialog child={d.child} /> : <SetDialog child={d.child} parent={d.parent} />}
      </Show>
    </dialog>
  );
}
