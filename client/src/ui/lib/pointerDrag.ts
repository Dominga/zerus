/**
 * In-page dragging without HTML5 drag and drop. On macOS (WKWebView) and
 * Windows (WebView2) Tauri's native file-drop handler answers every drag over
 * the window itself, so the page never receives dragover or drop for a drag
 * that starts in it: a copy cursor, no insertion line, nothing on release.
 * Finder and Explorer file drops keep using that handler (store/drops.ts).
 * Linux WebKitGTK passes in-page drags through and keeps HTML5 drag and drop.
 * A page may set `window.__ZERUS_POINTER_DRAG__` before loading to choose
 * (scripts/session-pointer-drag-smoke.html).
 */
export const POINTER_DRAG = (globalThis as { __ZERUS_POINTER_DRAG__?: boolean }).__ZERUS_POINTER_DRAG__
  ?? /Mac|iPhone|iPad|Win/.test(globalThis.navigator?.platform ?? "");

export interface PointerDrag {
  /** The pointer moved past the threshold: begin, or return false to leave it a click. */
  start(): boolean;
  /** Each move while dragging, and each autoscroll step, at the pointer's client position. */
  move(x: number, y: number): void;
  /** The pointer was released while dragging. */
  drop(x: number, y: number): void;
  /** Escape, a cancelled pointer or a lost window: nothing changes. */
  cancel(): void;
  /** Scrolls while the pointer is near its top or bottom edge. */
  scroller?(): { scrollTop: number; getBoundingClientRect(): { top: number; bottom: number } } | undefined;
}

/** The part of a pointer or key event a drag reads. */
interface PointerLike { clientX: number; clientY: number; pointerId: number; button?: number; isPrimary?: boolean; preventDefault(): void }
/** The window, or a test double of it. */
export type DragHost = Pick<Window, "addEventListener" | "removeEventListener" | "requestAnimationFrame" | "cancelAnimationFrame" | "setTimeout">;
const listen = (f: (event: never) => void) => f as unknown as EventListener;

export const THRESHOLD = 4;
const EDGE = 28, STEP = 12;

/** Follows one pointer from `down` until it is released, cancelled or Escape is pressed. */
export function pointerDrag(down: PointerLike, drag: PointerDrag, host: DragHost = window): void {
  if ((down.button ?? 0) !== 0 || down.isPrimary === false) return;
  const x0 = down.clientX, y0 = down.clientY;
  let x = x0, y = y0, started = false, frame = 0;
  const finish = () => {
    host.removeEventListener("pointermove", handlers.move, true);
    host.removeEventListener("pointerup", handlers.up, true);
    host.removeEventListener("pointercancel", handlers.cancel, true);
    host.removeEventListener("keydown", handlers.key, true);
    host.removeEventListener("blur", handlers.cancel);
    if (frame) host.cancelAnimationFrame(frame);
    frame = 0;
  };
  const scroll = () => {
    frame = 0;
    const el = drag.scroller?.();
    if (!started || !el) return;
    const box = el.getBoundingClientRect();
    const delta = y < box.top + EDGE ? -STEP : y > box.bottom - EDGE ? STEP : 0;
    if (!delta) return;
    const before = el.scrollTop;
    el.scrollTop += delta;
    if (el.scrollTop === before) return;
    drag.move(x, y);
    frame = host.requestAnimationFrame(scroll);
  };
  const move = (e: PointerLike) => {
    if (e.pointerId !== down.pointerId) return;
    x = e.clientX; y = e.clientY;
    if (!started) {
      if (Math.hypot(x - x0, y - y0) < THRESHOLD) return;
      if (!drag.start()) { finish(); return; }
      started = true;
    }
    e.preventDefault();
    drag.move(x, y);
    if (!frame) frame = host.requestAnimationFrame(scroll);
  };
  const up = (e: PointerLike) => {
    if (e.pointerId !== down.pointerId) return;
    finish();
    if (!started) return;
    // The click this release sends would select or open what was dragged.
    const swallow = (click: Event) => { click.stopPropagation(); click.preventDefault(); };
    host.addEventListener("click", swallow, { capture: true, once: true });
    host.setTimeout(() => host.removeEventListener("click", swallow, true), 0);
    drag.drop(e.clientX, e.clientY);
  };
  const cancel = () => { finish(); if (started) drag.cancel(); };
  const key = (e: { key: string; preventDefault(): void }) => { if (e.key === "Escape") { e.preventDefault(); cancel(); } };
  const handlers = { move: listen(move), up: listen(up), cancel: listen(cancel), key: listen(key) };
  host.addEventListener("pointermove", handlers.move, true);
  host.addEventListener("pointerup", handlers.up, true);
  host.addEventListener("pointercancel", handlers.cancel, true);
  host.addEventListener("keydown", handlers.key, true);
  host.addEventListener("blur", handlers.cancel);
}
