import { defaultRangeExtractor, type Range } from "@tanstack/solid-virtual";

export const ACTIVITY_OVERSCAN = 8;
export const ACTIVITY_RESIZE_OVERSCAN = 2;

/** Exclude retained media/selection/premeasure rows from width invalidation. */
export function activityMeasureWindow(range: Pick<Range, "startIndex" | "endIndex"> | null, count: number, overscan = ACTIVITY_OVERSCAN): number[] {
  return range ? defaultRangeExtractor({ ...range, count, overscan }) : [];
}

/** Height estimates only: mounted rows still get their exact DOM measurement.
 * Six width buckets, bounded by the currently loaded transcript's keys. Never
 * retain DOM nodes, Markdown bodies or a second copy of transcript content. */
export class ActivityHeights {
  private widths = new Map<number, Map<string, number>>();
  private bucket(width: number) { return Math.round(width / 32); }
  get(key: string, width: number): number | undefined { return this.widths.get(this.bucket(width))?.get(key); }
  set(key: string, width: number, height: number) {
    if (!(width > 0 && height > 0 && Number.isFinite(width) && Number.isFinite(height))) return;
    const bucket = this.bucket(width), values = this.widths.get(bucket) ?? new Map<string, number>();
    this.widths.delete(bucket); this.widths.set(bucket, values);
    values.set(key, height);
    if (this.widths.size > 6) this.widths.delete(this.widths.keys().next().value!);
  }
  invalidate(key: string) { for (const values of this.widths.values()) values.delete(key); }
  retain(keys: ReadonlySet<string>) {
    for (const values of this.widths.values()) for (const key of values.keys()) if (!keys.has(key)) values.delete(key);
  }
  clear() { this.widths.clear(); }
}

/** Per transcript update: a row whose content changed loses its estimates,
 * and only loaded rows keep any, so the cache stays bounded by the current
 * transcript (older pages, a trimmed tail or a new target drop the rest). */
export function syncActivityHeights(heights: ActivityHeights, previous: ReadonlyMap<string, string>, next: ReadonlyMap<string, string>) {
  for (const [key, value] of next) if (value !== previous.get(key)) heights.invalidate(key);
  heights.retain(new Set(next.keys()));
}

/** How long widths must stay unchanged before scrolling gets its overscan back. */
export const ACTIVITY_RESIZE_SETTLE_MS = 250;
export const activityOverscan = (resizing: boolean) => resizing ? ACTIVITY_RESIZE_OVERSCAN : ACTIVITY_OVERSCAN;

/** A width change uses the small resize overscan; ACTIVITY_OVERSCAN returns
 * ACTIVITY_RESIZE_SETTLE_MS after the last one, then `settled` runs once. */
export function createActivityResize(setResizing: (resizing: boolean) => void, settled: () => void,
  timers: { set(run: () => void, ms: number): unknown; clear(id: unknown): void } = {
    set: (run, ms) => setTimeout(run, ms), clear: (id) => clearTimeout(id as ReturnType<typeof setTimeout>) }) {
  let timer: unknown;
  return {
    resized() {
      setResizing(true);
      if (timer !== undefined) timers.clear(timer);
      timer = timers.set(() => { timer = undefined; setResizing(false); settled(); }, ACTIVITY_RESIZE_SETTLE_MS);
    },
    dispose() { if (timer !== undefined) timers.clear(timer); timer = undefined; },
  };
}
