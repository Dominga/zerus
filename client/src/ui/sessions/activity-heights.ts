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
