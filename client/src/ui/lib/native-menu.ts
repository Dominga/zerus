import { textEditor } from "./text-edit";

/** The webview's own context menu (Back, Forward, Stop, Reload on WebKitGTK)
 * would navigate or reload the whole app. Zerus menus call preventDefault
 * themselves; any other right-click is closed here, except where the native
 * menu is useful: editable text and a non-empty text selection (Copy). */
export function nativeMenuAllowed(event: MouseEvent, selection = window.getSelection()): boolean {
  if (textEditor(event.target)) return true;
  if (event.target instanceof Element && event.target.closest("[contenteditable=''], [contenteditable='true']")) return true;
  return !!selection && !selection.isCollapsed && selection.toString().trim() !== "";
}

export function installNativeMenuGuard(target: Window = window): () => void {
  const menu = (event: MouseEvent) => {
    if (event.defaultPrevented || nativeMenuAllowed(event)) return;
    event.preventDefault();
  };
  target.addEventListener("contextmenu", menu);
  return () => target.removeEventListener("contextmenu", menu);
}
