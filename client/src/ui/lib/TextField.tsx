import { createMemo, createRenderEffect, createSignal, onCleanup, Show, splitProps, type JSX } from "solid-js";
import { isServer } from "solid-js/web";
import { inboundEdit, replaceText, setComposing, type TextEditor } from "./text-edit";

type Identity = { historyKey?: string | number; valueRevision?: number };
type InputProps = JSX.InputHTMLAttributes<HTMLInputElement> & Identity;
type AreaProps = JSX.TextareaHTMLAttributes<HTMLTextAreaElement> & Identity;

/** DOM owns ordinary typing. Same-value model echoes never rewrite it. A real
 * model edit uses the native editing pipeline while focused; a new context or
 * an out-of-focus replacement gets a fresh node/history, rather than stale undo
 * ranges from another value or another draft. */
function field(props: InputProps | AreaProps, area: boolean) {
  const [local, rest] = splitProps(props, ["value", "ref", "historyKey", "valueRevision"]);
  if (isServer) return area
    ? <textarea {...rest as JSX.TextareaHTMLAttributes<HTMLTextAreaElement>} value={local.value} />
    : <input {...rest as JSX.InputHTMLAttributes<HTMLInputElement>} value={local.value} />;
  let current: TextEditor | undefined;
  const [generation, bump] = createSignal(0);
  const identity = createMemo((old: { key: unknown; generation: number } | undefined) => {
    const key = local.historyKey, next = generation();
    if (old && old.key !== key && current && document.activeElement === current)
      restore = { focused: true, start: null, end: null, direction: null };
    return old && old.key === key && old.generation === next ? old : { key, generation: next };
  });
  let restore: { focused: boolean; start: number | null; end: number | null; direction: "forward" | "backward" | "none" | null } | undefined;
  const bind = (editor: TextEditor) => {
    current = editor;
    let edited = false, composing = false;
    let lastIncoming: string | undefined, lastRevision: number | undefined;
    let suppressChange = false;
    const [composition, setComposition] = createSignal(false);
    let timer: ReturnType<typeof setTimeout> | undefined;
    const input = (event: Event) => {
      if (inboundEdit(editor)) { event.stopImmediatePropagation(); return; }
      edited = true; suppressChange = false;
    };
    const change = (event: Event) => { if (suppressChange) { suppressChange = false; event.stopImmediatePropagation(); } };
    const begin = () => { composing = true; setComposing(editor, true); setComposition(true); };
    const end = () => {
      clearTimeout(timer);
      // The final IME input follows compositionend. Let it reach the model
      // before considering any pending external value.
      timer = setTimeout(() => { composing = false; setComposing(editor, false); setComposition(false); }, 0);
    };
    editor.addEventListener("input", input, true);
    editor.addEventListener("change", change, true);
    editor.addEventListener("compositionstart", begin);
    editor.addEventListener("compositionend", end);
    onCleanup(() => {
      if (current === editor) current = undefined;
      clearTimeout(timer); setComposing(editor, false);
      editor.removeEventListener("input", input, true);
      editor.removeEventListener("change", change, true);
      editor.removeEventListener("compositionstart", begin); editor.removeEventListener("compositionend", end);
    });
    createRenderEffect(() => {
      const value = String(local.value ?? ""), revision = local.valueRevision;
      if (composition() || composing || (value === lastIncoming && revision === lastRevision)) return;
      lastIncoming = value; lastRevision = revision;
      if (editor.value === value) return;
      if (replaceText(editor, value, false)) { edited = true; suppressChange = true; return; }
      if (edited) {
        restore = { focused: document.activeElement === editor, start: editor.selectionStart, end: editor.selectionEnd, direction: editor.selectionDirection };
        bump((n) => n + 1);
      } else editor.value = value;
    });
    const ref = local.ref;
    if (typeof ref === "function") ref(editor as HTMLInputElement & HTMLTextAreaElement);
    const selection = restore; restore = undefined;
    if (selection?.focused) queueMicrotask(() => {
      if (!editor.isConnected) return;
      editor.focus({ preventScroll: true });
      try { if (selection.start !== null) editor.setSelectionRange(selection.start, selection.end ?? selection.start, selection.direction ?? "none"); } catch { /* Native non-selection input. */ }
    });
  };
  return <Show when={identity()} keyed>{(_identity) => area
    ? <textarea {...rest as JSX.TextareaHTMLAttributes<HTMLTextAreaElement>} autofocus={generation() === 0 && !!rest.autofocus} ref={bind} />
    : <input {...rest as JSX.InputHTMLAttributes<HTMLInputElement>} autofocus={generation() === 0 && !!rest.autofocus} ref={bind} />}</Show>;
}
export const TextInput = (props: InputProps) => field(props, false);
export const TextArea = (props: AreaProps) => field(props, true);
