import { animationFrames, type AnimationFrames } from "./activity-end";

export interface ScrollIdleHost extends AnimationFrames { later(run: () => void, ms: number): () => void }
const browser: ScrollIdleHost = { ...animationFrames, later(run, ms) { const id = setTimeout(run, ms); return () => clearTimeout(id); } };

/** The compositor owns a gesture and its momentum until scrolling is quiet.
 * Keep only the latest pending layout, then verify two stable frames before
 * handing scroll ownership back to the reading anchor or end pin. */
export function createActivityScroll(readTop: () => number, settle: (flush: () => void) => void, host: ScrollIdleHost = browser) {
  let active = false, held = false, disposed = false;
  let pending: (() => void) | undefined, cancelTimer: (() => void) | undefined, frame: number | undefined;
  const cancel = () => { cancelTimer?.(); cancelTimer = undefined; if (frame !== undefined) host.cancel(frame); frame = undefined; };
  const arm = () => {
    cancel();
    if (held || disposed) return;
    cancelTimer = host.later(() => {
      cancelTimer = undefined;
      const top = readTop();
      frame = host.request(() => {
        frame = undefined;
        if (top !== readTop()) { arm(); return; }
        frame = host.request(() => {
          frame = undefined;
          if (top !== readTop()) { arm(); return; }
          active = false;
          settle(() => { const apply = pending; pending = undefined; apply?.(); });
        });
      });
    }, 250);
  };
  const intent = () => { if (!disposed) { active = true; arm(); } };
  return {
    active: () => active,
    intent,
    moved() { if (active) intent(); },
    hold() { held = true; intent(); },
    release() { if (held) { held = false; intent(); } },
    update(apply: () => void) { if (disposed) return; if (active) pending = apply; else apply(); },
    reset() { cancel(); active = held = false; pending = undefined; },
    dispose() { cancel(); active = held = false; pending = undefined; disposed = true; },
  };
}
