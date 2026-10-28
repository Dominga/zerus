import { For, Show, createMemo, createSignal } from "solid-js";
import {
  COMMANDS, assignable, command, comboFromEvent, conflicts, customized, keys, nativeText, overlaps, platform, reset,
  resetAll, setKeys, terminalRule, terminalSafe, type Sequence,
} from "../../../store/shortcuts";
import { SectionHead } from "./ui";

// Settings > Keyboard (tray/src/KeyboardSettings.cpp): every command by group
// with its two chords, Reset and Reset all, and the change dialog with Qt's
// assignability rules and reassignment of conflicting chords. Chords appear
// here and nowhere else in the client.

/** QLocale().createSeparatedList for English: "A", "A and B", "A, B and C". */
export function separatedList(names: string[]): string {
  if (names.length < 2) return names.join("");
  return `${names.slice(0, -1).join(", ")} and ${names[names.length - 1]}`;
}

const groups = () => {
  const order: string[] = [];
  for (const c of COMMANDS) if (!order.includes(c.group)) order.push(c.group);
  return order.map((group) => ({ group, commands: COMMANDS.filter((c) => c.group === group) }));
};

/** A chord as the table shows it: native text, with ✓ when it works in Terminal. */
function Chord(props: { id: string; slot: number }) {
  const sequence = () => keys(props.id)[props.slot];
  const works = () => command(props.id)?.scope === "window" && !!sequence() && terminalSafe(sequence()![0]);
  return (
    <Show when={sequence()}>
      {(s) => <span title={works() ? "Works in Terminal" : undefined}>{nativeText(s())}{works() ? " ✓" : ""}</span>}
    </Show>
  );
}

export function KeyboardSection() {
  const [selected, setSelected] = createSignal<string | undefined>();
  const [editing, setEditing] = createSignal<{ id: string; slot: number } | undefined>();
  const [confirmAll, setConfirmAll] = createSignal(false);
  const list = createMemo(groups);
  return (
    <>
      <SectionHead title="Keyboard" />
      <div class="set-table-wrap kb-wrap">
        <table class="set-table kb-table" aria-label="Keyboard shortcuts">
          <thead><tr><th>Command</th><th>Shortcut</th><th>Alternate</th></tr></thead>
          <tbody>
            <For each={list()}>
              {(g) => (
                <>
                  <tr class="set-table-group"><td colSpan={3}>{g.group}</td></tr>
                  <For each={g.commands}>
                    {(c) => (
                      <tr class="kb-row" classList={{ on: selected() === c.id }} tabindex="0" aria-selected={selected() === c.id}
                        onClick={() => setSelected(c.id)} onFocus={() => setSelected(c.id)}
                        onKeyDown={(e) => { if (e.key === "Enter") { e.preventDefault(); setEditing({ id: c.id, slot: 0 }); } }}>
                        <td class="kb-cmd" classList={{ custom: customized(c.id) }}>{c.title}</td>
                        <td class="kb-chord" onDblClick={() => setEditing({ id: c.id, slot: 0 })}><Chord id={c.id} slot={0} /></td>
                        <td class="kb-chord" onDblClick={() => setEditing({ id: c.id, slot: 1 })}><Chord id={c.id} slot={1} /></td>
                      </tr>
                    )}
                  </For>
                </>
              )}
            </For>
          </tbody>
        </table>
      </div>
      <div class="set-actions">
        <button class="btn" disabled={!selected()} title="Restore the default chords of the selected command" onClick={() => { const id = selected(); if (id) reset(id); }}>Reset</button>
        <button class="btn" onClick={() => setConfirmAll(true)}>Reset all</button>
      </div>
      <p class="set-foot">{terminalRule()} Chords marked ✓ work in Terminal.</p>
      <p class="set-foot">Double-click a shortcut, or press Enter on it, to change it.</p>
      <Show when={editing()}>{(e) => <ChangeDialog id={e().id} slot={e().slot} close={() => setEditing(undefined)} />}</Show>
      <Show when={confirmAll()}>
        <Question title="Reset all shortcuts" text="Restore the default chords of every command?"
          answer={(yes) => { setConfirmAll(false); if (yes) resetAll(); }} />
      </Show>
    </>
  );
}

/** QMessageBox::question: Yes or No. */
function Question(props: { title: string; text: string; answer: (yes: boolean) => void }) {
  return (
    <dialog class="dlg kb-question" ref={(el) => queueMicrotask(() => el.showModal())} aria-labelledby="kb-question-title"
      onCancel={(e) => { e.preventDefault(); props.answer(false); }}>
      <div class="dlg-head"><h2 id="kb-question-title">{props.title}</h2></div>
      <div class="dlg-body">{props.text}</div>
      <div class="dlg-foot">
        <span class="spacer" />
        <button type="button" class="btn" onClick={() => props.answer(false)}>No</button>
        <button type="button" class="btn primary" onClick={() => props.answer(true)}>Yes</button>
      </div>
    </dialog>
  );
}

/** Change shortcut (KeyboardSettings::editChord). */
function ChangeDialog(props: { id: string; slot: number; close: () => void }) {
  const title = command(props.id)?.title ?? props.id;
  const [recorded, setRecorded] = createSignal<Sequence>(keys(props.id)[props.slot] ?? []);
  const [reason, setReason] = createSignal("");
  const [asking, setAsking] = createSignal<string[] | undefined>();
  let field!: HTMLDivElement;

  // QKeySequenceEdit with at most two chords: a third key starts over.
  const record = (e: KeyboardEvent) => {
    if (e.key === "Tab" && !e.ctrlKey && !e.metaKey && !e.altKey) return;
    const combo = comboFromEvent(e);
    e.preventDefault();
    e.stopPropagation();
    if (!combo) return;
    setRecorded((current) => (current.length >= 2 ? [combo] : [...current, combo]));
    setReason("");
  };

  const apply = () => {
    const sequence = recorded();
    let current = keys(props.id);
    if (!sequence.length) {
      if (props.slot < current.length) current = current.filter((_, i) => i !== props.slot);
    } else if (props.slot < current.length) {
      // The other slot goes when it is the same chord or a prefix either way.
      current = current.map((key, i) => (i === props.slot ? sequence : key)).filter((key, i) => i === props.slot || !overlaps(key, sequence));
    } else {
      current = [...current.filter((key) => !overlaps(key, sequence)), sequence];
    }
    setKeys(props.id, current);
    props.close();
  };

  const accept = () => {
    const sequence = recorded();
    const why = sequence.length ? assignable(sequence, platform) : undefined;
    if (why) { setReason(why); return; }
    const others = conflicts(props.id, sequence);
    if (others.length) { setAsking(others); return; }
    apply();
  };

  // Every other command whose chord equals the sequence or is a prefix
  // either way loses that chord, after one question naming them all.
  const reassign = (yes: boolean) => {
    const others = asking() ?? [];
    setAsking(undefined);
    if (!yes) return;
    const sequence = recorded();
    for (const other of others) setKeys(other, keys(other).filter((key) => !overlaps(key, sequence)));
    apply();
  };

  return (
    <>
      <dialog class="dlg kb-edit" ref={(el) => queueMicrotask(() => { el.showModal(); field.focus(); })} aria-labelledby="kb-edit-title"
        onCancel={(e) => { e.preventDefault(); props.close(); }}>
        <div class="dlg-head"><h2 id="kb-edit-title">Change shortcut</h2></div>
        <div class="dlg-body">
          <p>Press the new shortcut for {title}.</p>
          <div class="kb-capture-row">
            <div ref={field} class="dlg-input kb-capture" tabindex="0" role="textbox" aria-label={`Shortcut for ${title}`}
              data-shortcut-context="capture" onKeyDown={record}>{nativeText(recorded())}</div>
            <button type="button" class="btn" onClick={() => { setRecorded([]); setReason(""); field.focus(); }}>Clear</button>
          </div>
          <Show when={reason()}><div class="dlg-error">{reason()}</div></Show>
        </div>
        <div class="dlg-foot">
          <span class="spacer" />
          <button type="button" class="btn" onClick={props.close}>Cancel</button>
          <button type="button" class="btn primary" onClick={accept}>OK</button>
        </div>
      </dialog>
      <Show when={asking()}>
        {(others) => (
          <Question title="Shortcut in use" text={`Already used by ${separatedList(others().map((id) => command(id)?.title ?? id))}. Reassign?`} answer={reassign} />
        )}
      </Show>
    </>
  );
}
