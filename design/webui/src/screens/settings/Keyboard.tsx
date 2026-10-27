import { For, Show } from "solid-js";
import { setDialog } from "../../state";
import { SHORTCUTS } from "../../mock/settings-extra";
import { chords, setChords, chordText, isCustomized, resetChords, selectedCommand, setSelectedCommand, setEditing, isMac } from "./store";
import { SectionHead, Actions } from "./ui";

// Settings → Keyboard (KeyboardSettings.cpp, Shortcuts.cpp). Chords use the
// portable notation: "Ctrl" is ⌘ and "Meta" the Control key on macOS.

const GROUPS = [...new Set(SHORTCUTS.map((c) => c.group))];

function parse(chord: string) {
  const m = /^((?:(?:Ctrl|Shift|Alt|Meta)\+)*)(.+)$/.exec(chord.trim());
  const mods = new Set((m?.[1] ?? "").split("+").filter(Boolean));
  return { mods, key: m?.[2] ?? "" };
}
const only = (mods: Set<string>, ...want: string[]) => mods.size === want.length && want.every((w) => mods.has(w));

/** Shortcuts::terminalSafe: whether a window chord still reaches Zerus while Terminal has focus. */
export function terminalSafe(sequence: string): boolean {
  const { mods, key } = parse(sequence.split(", ")[0]);
  if (isMac) {
    if (mods.has("Ctrl")) return !(only(mods, "Ctrl") && (key === "C" || key === "V"));
    return (only(mods, "Meta") || only(mods, "Meta", "Shift")) && (key === "Tab" || key === "`");
  }
  if (!only(mods, "Ctrl") && !only(mods, "Ctrl", "Shift")) return false;
  if (/^[0-9]$/.test(key) || ["Tab", "PgUp", "PgDown", "`", ","].includes(key)) return true;
  return only(mods, "Ctrl", "Shift") && (key === "Return" || key === "Enter");
}

const selectionKey = (mods: Set<string>, key: string) => only(mods, "Ctrl") && (key === "X" || key === "C");
function editingKey(mods: Set<string>, key: string) {
  if (only(mods, "Ctrl") && ["C", "X", "V", "Z", "Y", "A", "Ins"].includes(key)) return true;
  if (only(mods, "Ctrl", "Shift") && key === "Z") return true;
  const chordMod = mods.has("Ctrl") || mods.has("Alt") || mods.has("Meta");
  return chordMod && ["Left", "Right", "Home", "End", "Backspace", "Del"].includes(key);
}

/** Shortcuts::assignable: an empty reason means the sequence can be bound. */
export function assignReason(sequence: string): string {
  const parts = sequence.split(", ").filter(Boolean);
  if (!parts.length) return "Press a key combination.";
  if (parts.length > 2) return "Use one key combination or a sequence of two.";
  for (const [i, part] of parts.entries()) {
    const { mods, key } = parse(part);
    if (!key || ["Ctrl", "Shift", "Alt", "Meta"].includes(key)) return "Press a key together with the modifiers.";
    if ((key === "F10" && only(mods, "Shift")) || key === "Menu") return "This key opens context menus.";
    const chordMod = mods.has("Ctrl") || mods.has("Alt") || mods.has("Meta");
    if (!chordMod && !/^F\d{1,2}$/.test(key))
      return isMac ? "Keys without ⌘, ⌃ or ⌥ are used for typing and navigation." : "Keys without Ctrl or Alt are used for typing and navigation.";
    if (!isMac && only(mods, "Ctrl", "Shift") && (key === "C" || key === "V")) return "Terminal uses this key to copy and paste.";
    if (editingKey(mods, key) && !(i === 0 && parts.length > 1 && selectionKey(mods, key))) return "Text fields use this key for editing.";
  }
  return "";
}

/** Equal, or a prefix either way ("Ctrl+X" and "Ctrl+X, Ctrl+S"). */
export const overlaps = (a: string, b: string) => a === b || a.startsWith(`${b}, `) || b.startsWith(`${a}, `);

export function conflictsOf(id: string, sequence: string): string[] {
  if (!sequence) return [];
  return SHORTCUTS.filter((c) => c.id !== id && chords[c.id].some((k) => overlaps(k, sequence))).map((c) => c.id);
}

/** Apply an accepted chord to its slot, after other commands gave up overlapping chords. */
export function assignChord(id: string, slot: number, sequence: string, others: string[]) {
  for (const other of others) setChords(other, chords[other].filter((k) => !overlaps(k, sequence)));
  let keys = [...chords[id]];
  if (!sequence) { if (slot < keys.length) keys.splice(slot, 1); }
  else if (slot < keys.length) {
    keys[slot] = sequence;
    keys = keys.filter((k, i) => i === slot || !overlaps(k, sequence));
  } else {
    keys = keys.filter((k) => !overlaps(k, sequence));
    keys.push(sequence);
  }
  setChords(id, keys);
}

export const commandTitle = (id: string) => SHORTCUTS.find((c) => c.id === id)?.title ?? id;

export function KeyboardSection() {
  const edit = (id: string, slot: number) => {
    setSelectedCommand(id);
    setEditing({ id, slot, captured: chords[id][slot] ?? "", conflicts: [] });
    setDialog("settings-shortcut-edit");
  };
  const cell = (id: string, slot: number, window: boolean) => {
    const k = () => chords[id][slot];
    return (
      <td class="kb-chord" onDblClick={() => edit(id, slot)}>
        <Show when={k()}>
          <span class="kbd">{chordText(k()!)}</span>
          <Show when={window && terminalSafe(k()!)}><span class="kb-term" title="Works in Terminal">✓</span></Show>
        </Show>
      </td>
    );
  };
  const move = (e: KeyboardEvent) => {
    if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
    e.preventDefault();
    const i = SHORTCUTS.findIndex((c) => c.id === selectedCommand());
    const next = SHORTCUTS[Math.min(SHORTCUTS.length - 1, Math.max(0, i + (e.key === "ArrowDown" ? 1 : -1)))];
    setSelectedCommand(next.id);
    (document.querySelector(`[data-cmd="${next.id}"]`) as HTMLElement | null)?.focus();
  };
  return (
    <>
      <SectionHead title="Keyboard" />
      <div class="set-table-wrap kb-wrap">
        <table class="set-table kb-table" aria-label="Keyboard shortcuts" onKeyDown={move}>
          <thead><tr><th scope="col">Command</th><th scope="col">Shortcut</th><th scope="col">Alternate</th></tr></thead>
          <tbody>
            <For each={GROUPS}>
              {(group) => (
                <>
                  <tr class="set-table-group"><td colspan={3}>{group}</td></tr>
                  <For each={SHORTCUTS.filter((c) => c.group === group)}>
                    {(c) => (
                      <tr data-cmd={c.id} tabindex={0} classList={{ on: selectedCommand() === c.id }} aria-selected={selectedCommand() === c.id}
                        onClick={() => setSelectedCommand(c.id)} onKeyDown={(e) => { if (e.key === "Enter") edit(c.id, 0); }}>
                        <td class="kb-cmd" classList={{ custom: isCustomized(c.id) }} onDblClick={() => edit(c.id, 0)}>{c.title}</td>
                        {cell(c.id, 0, c.scope === "window")}
                        {cell(c.id, 1, c.scope === "window")}
                      </tr>
                    )}
                  </For>
                </>
              )}
            </For>
          </tbody>
        </table>
      </div>
      <Actions>
        <button class="btn" disabled={!selectedCommand()} title="Restore the default chords of the selected command" onClick={() => selectedCommand() && resetChords(selectedCommand()!)}>Reset</button>
        <button class="btn" onClick={() => setDialog("settings-shortcut-reset-all")}>Reset all</button>
      </Actions>
      <p class="set-foot">
        {isMac
          ? "In Terminal, a command's chord reaches Zerus only when it uses ⌘ (except ⌘C and ⌘V) or is ⌃Tab, ⌃⇧Tab, ⌃` or ⌃⇧`. Every other ⌃ chord and every ⌥ chord goes to the agent, bound or not."
          : "In Terminal, a command's chord reaches Zerus only when it is Ctrl or Ctrl+Shift with a digit, Tab, Page Up, Page Down, backtick or comma, or Ctrl+Shift+Enter. Everything else goes to the agent, bound or not."}{" "}
        Chords marked ✓ work in Terminal.
      </p>
      <p class="set-foot">Double-click a shortcut, or press Enter on it, to change it.</p>
    </>
  );
}
