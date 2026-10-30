/** Layout settling at Activity's end, shared by initial placement and follow growth. */
export interface ScrollGeometry { top: number; height: number; viewport: number }
export interface AnimationFrames { request(callback: () => void): number; cancel(id: number): void }
export const animationFrames: AnimationFrames = { request: (callback) => requestAnimationFrame(callback), cancel: (id) => cancelAnimationFrame(id) };

/**
 * Pin after rendering, then verify once in the next frame. ResizeObserver's
 * later content/viewport changes coalesce into another pair of frames.
 * Layout and browser scroll anchoring never revoke the pin. The owner
 * releases it on explicit user scroll intent or before restoring an older page.
 */
export function createActivityEnd(read: () => ScrollGeometry, writeEnd: () => void, frames: AnimationFrames = animationFrames) {
  let active = false, disposed = false, epoch = 0;
  let frame: number | undefined;
  let last: ScrollGeometry | undefined;
  const cancel = () => { if (frame !== undefined) frames.cancel(frame); frame = undefined; };
  const schedule = (verify = true) => {
    if (!active || disposed || frame !== undefined) return;
    const mine = epoch;
    frame = frames.request(() => {
      if (mine !== epoch || !active || disposed) return;
      frame = undefined;
      writeEnd();
      last = read();
      if (verify) schedule(false);
    });
  };
  const release = () => { active = false; epoch++; cancel(); last = undefined; };
  return {
    active: () => active,
    pin() { if (disposed) return; release(); active = true; schedule(); },
    changed() { schedule(); },
    /** True while a scroll belongs to initial placement or a layout change. */
    scrolled() {
      if (!active || disposed) return false;
      const now = read();
      if (!last || now.height !== last.height || now.viewport !== last.viewport
        || now.height - now.top - now.viewport > 1) schedule();
      return true;
    },
    release,
    dispose() { release(); disposed = true; },
  };
}
