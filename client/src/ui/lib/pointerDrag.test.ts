import { describe, expect, it } from "vitest";
import { pointerDrag, THRESHOLD, type DragHost, type PointerDrag } from "./pointerDrag";

/** A window double: listeners on one EventTarget and manually run animation frames. */
function host() {
  const target = new EventTarget();
  const frames = new Map<number, () => void>();
  const timers: (() => void)[] = [];
  let next = 1;
  // A Window accepts the boolean capture shorthand. Node 24's EventTarget
  // removes listeners only with an options object; preserve the Window API.
  const options = <T extends EventListenerOptions>(value?: boolean | T) => typeof value === "boolean" ? { capture: value } : value;
  const win = {
    addEventListener: (type: string, listener: EventListenerOrEventListenerObject, value?: boolean | AddEventListenerOptions) => target.addEventListener(type, listener, options(value)),
    removeEventListener: (type: string, listener: EventListenerOrEventListenerObject, value?: boolean | EventListenerOptions) => target.removeEventListener(type, listener, options(value)),
    requestAnimationFrame: (callback: () => void) => { frames.set(next, callback); return next++; },
    cancelAnimationFrame: (handle: number) => { frames.delete(handle); },
    setTimeout: (callback: () => void) => { timers.push(callback); return 0; },
  } as unknown as DragHost;
  const fire = (type: string, fields: Record<string, unknown> = {}) => {
    const event = Object.assign(new Event(type, { cancelable: true }), { pointerId: 1, clientX: 0, clientY: 0, ...fields });
    target.dispatchEvent(event);
    return event;
  };
  const frame = () => { const pending = [...frames.values()]; frames.clear(); pending.forEach((f) => f()); };
  const tick = () => timers.splice(0).forEach((f) => f());
  return { win, fire, frame, tick, frames };
}
const down = (fields: Record<string, unknown> = {}) => ({ pointerId: 1, clientX: 10, clientY: 10, button: 0, isPrimary: true, preventDefault() {}, ...fields });
function recorder(start = true) {
  const calls: string[] = [];
  const drag: PointerDrag = {
    start: () => { calls.push("start"); return start; },
    move: (x, y) => { calls.push(`move ${x},${y}`); },
    drop: (x, y) => { calls.push(`drop ${x},${y}`); },
    cancel: () => { calls.push("cancel"); },
  };
  return { drag, calls };
}

describe("an in-page pointer drag (macOS and Windows webviews)", () => {
  it("the Window double removes only the matching listener/capture and preserves once", () => {
    const h = host(); let calls = 0;
    const listener = () => { calls++; };
    h.win.addEventListener("fixture", listener, true);
    h.win.addEventListener("fixture", listener, false);
    h.fire("fixture"); expect(calls).toBe(2);
    h.win.removeEventListener("fixture", listener, true);
    h.fire("fixture"); expect(calls).toBe(3);
    h.win.removeEventListener("fixture", listener, false);
    h.fire("fixture"); expect(calls).toBe(3);
    h.win.addEventListener("fixture", listener, { capture: true, once: true });
    h.fire("fixture"); expect(calls).toBe(4);
    h.fire("fixture"); expect(calls).toBe(4);
  });
  it("is a click until the pointer moves past the threshold", () => {
    const h = host(), r = recorder();
    pointerDrag(down(), r.drag, h.win);
    h.fire("pointermove", { clientX: 10 + THRESHOLD - 1, clientY: 10 });
    const up = h.fire("pointerup", { clientX: 12, clientY: 10 });
    expect(r.calls).toEqual([]);
    expect(h.fire("click").defaultPrevented).toBe(false);
    expect(up.defaultPrevented).toBe(false);
  });
  it("starts once, follows the pointer, drops where it is released and swallows that click only", () => {
    const h = host(), r = recorder();
    pointerDrag(down(), r.drag, h.win);
    const moved = h.fire("pointermove", { clientX: 10, clientY: 20 });
    h.fire("pointermove", { clientX: 11, clientY: 40 });
    h.fire("pointerup", { clientX: 11, clientY: 42 });
    expect(r.calls).toEqual(["start", "move 10,20", "move 11,40", "drop 11,42"]);
    expect(moved.defaultPrevented).toBe(true);
    const click = h.fire("click");
    expect([click.defaultPrevented, click.cancelBubble]).toEqual([true, true]);
    h.tick();
    expect(h.fire("click").defaultPrevented).toBe(false);
    // Released: later pointer events belong to nothing.
    h.fire("pointermove", { clientX: 50, clientY: 50 });
    h.fire("pointerup", { clientX: 50, clientY: 50 });
    expect(r.calls).toHaveLength(4);
  });
  it("a refused start leaves the press a plain click", () => {
    const h = host(), r = recorder(false);
    pointerDrag(down(), r.drag, h.win);
    h.fire("pointermove", { clientX: 30, clientY: 30 });
    h.fire("pointerup", { clientX: 30, clientY: 30 });
    expect(r.calls).toEqual(["start"]);
    expect(h.fire("click").defaultPrevented).toBe(false);
  });
  it("Escape, a cancelled pointer or a lost window cancel without a drop", () => {
    for (const end of [["keydown", { key: "Escape" }], ["pointercancel", {}], ["blur", {}]] as const) {
      const h = host(), r = recorder();
      pointerDrag(down(), r.drag, h.win);
      h.fire("pointermove", { clientX: 10, clientY: 30 });
      h.fire(end[0], end[1]);
      h.fire("pointerup", { clientX: 10, clientY: 30 });
      expect(r.calls, end[0]).toEqual(["start", "move 10,30", "cancel"]);
    }
  });
  it("follows only its own primary pointer", () => {
    const h = host(), r = recorder();
    pointerDrag(down(), r.drag, h.win);
    h.fire("pointermove", { pointerId: 2, clientX: 90, clientY: 90 });
    h.fire("pointerup", { pointerId: 2, clientX: 90, clientY: 90 });
    expect(r.calls).toEqual([]);
    // Another pointer's release while dragging leaves this drag running.
    h.fire("pointermove", { clientX: 10, clientY: 30 });
    h.fire("pointerup", { pointerId: 2, clientX: 10, clientY: 30 });
    h.fire("pointerup", { clientX: 10, clientY: 31 });
    expect(r.calls).toEqual(["start", "move 10,30", "drop 10,31"]);
    const secondary = host(), s = recorder();
    pointerDrag(down({ button: 2 }), s.drag, secondary.win);
    secondary.fire("pointermove", { clientX: 90, clientY: 90 });
    expect(s.calls).toEqual([]);
  });
  it("a lost release cancels, and the pressed element holds the pointer while dragging", () => {
    const h = host(), r = recorder(), held: string[] = [];
    const target = { setPointerCapture: (id: number) => held.push(`set ${id}`), releasePointerCapture: (id: number) => held.push(`release ${id}`) };
    pointerDrag(down({ target }), r.drag, h.win);
    h.fire("pointermove", { clientX: 10, clientY: 30, buttons: 1 });
    expect(held).toEqual(["set 1"]);
    // Released outside the window: the next move has no button pressed.
    h.fire("pointermove", { clientX: 12, clientY: 40, buttons: 0 });
    h.fire("pointerup", { clientX: 12, clientY: 40 });
    expect(r.calls).toEqual(["start", "move 10,30", "cancel"]);
    expect(held).toEqual(["set 1", "release 1"]);
  });
  it("scrolls its scroller while the pointer is near an edge, updating the target", () => {
    const h = host(), r = recorder();
    const scroller = { scrollTop: 100, getBoundingClientRect: () => ({ top: 0, bottom: 400 }) };
    pointerDrag(down({ clientY: 200 }), { ...r.drag, scroller: () => scroller }, h.win);
    h.fire("pointermove", { clientX: 10, clientY: 395 });
    h.frame();
    expect(scroller.scrollTop).toBeGreaterThan(100);
    expect(r.calls.filter((c) => c === "move 10,395").length).toBe(2);
    h.fire("pointermove", { clientX: 10, clientY: 3 });
    const top = scroller.scrollTop;
    h.frame();
    expect(scroller.scrollTop).toBeLessThan(top);
    h.fire("pointerup", { clientX: 10, clientY: 3 });
    expect(h.frames.size).toBe(0);
  });
});

// Finder and Explorer file drops need Tauri's native file-drop handler, which
// is exactly what keeps in-page HTML5 drags from macOS and Windows pages.
const fs = (await import(/* @vite-ignore */ ["node", "fs"].join(":"))) as { readFileSync(path: URL, encoding: "utf8"): string };
const source = (path: string) => fs.readFileSync(new URL(path, import.meta.url), "utf8");

describe("file drops keep the native handler", () => {
  it("the window keeps Tauri's file-drop handler and the page still listens to it", () => {
    expect(source("../../../src-tauri/src/lib.rs")).not.toMatch(/disable_drag_drop_handler|drag_and_drop\(false\)/);
    const conf = JSON.parse(source("../../../src-tauri/tauri.conf.json")) as { app?: { windows?: { dragDropEnabled?: boolean }[] } };
    expect((conf.app?.windows ?? []).every((w) => w.dragDropEnabled !== false)).toBe(true);
    expect(source("../../store/drops.ts")).toContain("onDragDropEvent");
  });
});
