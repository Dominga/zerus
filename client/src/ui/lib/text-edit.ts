/** Native text transactions keep WebKit's undo manager usable. Directly setting
 * a changed value during editing leaves stale undo ranges behind. */
export type TextEditor = HTMLInputElement | HTMLTextAreaElement;
const composing = new WeakSet<TextEditor>();
const inbound = new WeakSet<TextEditor>();

export function textEditor(target: EventTarget | null): TextEditor | undefined {
  if (!(target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement)) return;
  if (target.disabled || target.readOnly || target.closest(".xterm, [data-shortcut-context=terminal]")) return;
  if (target instanceof HTMLInputElement && ["button", "checkbox", "radio", "range", "file", "hidden", "submit", "reset", "color", "date", "datetime-local", "month", "time", "week"].includes(target.type)) return;
  return target;
}

/** A user edit emits the usual input event. A value already accepted by the
 * model must not echo another edit back through the bridge. */
export function replaceText(editor: TextEditor, value: string, notify = true): boolean {
  if (editor.value === value) return true;
  if (document.activeElement !== editor || !textEditor(editor) || composing.has(editor)) return false;
  if (typeof document.execCommand !== "function") return false;
  if (!notify) inbound.add(editor);
  try {
    editor.select();
    const accepted = document.execCommand("insertText", false, value);
    return accepted && editor.value === value;
  } finally {
    inbound.delete(editor);
  }
}

export const inboundEdit = (editor: TextEditor) => inbound.has(editor);
export function setComposing(editor: TextEditor, active: boolean) {
  if (active) composing.add(editor); else composing.delete(editor);
}

/** The shortcut router reserves these combinations for text editing. Calling
 * the native command also supplies Ctrl+Y on engines whose default is Shift+Z. */
export function installTextEditingKeys(target: Window = window, mac = /Mac/.test(navigator.platform)): () => void {
  const key = (event: KeyboardEvent) => {
    if (event.defaultPrevented || event.isComposing || event.keyCode === 229 || event.altKey) return;
    const modified = mac ? event.metaKey && !event.ctrlKey : event.ctrlKey && !event.metaKey;
    if (!modified) return;
    const letter = /^[a-z]$/i.test(event.key) ? event.key.toLowerCase() : event.code === "KeyZ" ? "z" : event.code === "KeyY" ? "y" : "";
    const command = letter === "z" ? event.shiftKey ? "redo" : "undo" : letter === "y" && !event.shiftKey ? "redo" : undefined;
    const editor = textEditor(event.target);
    if (!command || !editor || composing.has(editor) || typeof document.queryCommandEnabled !== "function" || !document.queryCommandEnabled(command)) return;
    event.preventDefault();
    document.execCommand(command);
  };
  target.addEventListener("keydown", key);
  return () => target.removeEventListener("keydown", key);
}
