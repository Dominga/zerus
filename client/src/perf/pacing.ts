// Frame pacing: requestAnimationFrame intervals and the spec's pass/fail criteria.
//
// Spec (docs/superpowers/specs/2026-10-27-tauri-web-ui-design.md, "macOS pass/fail"):
// T = 1000 / display Hz. rAF on a light page and during a CSS transform animation:
// mean interval within T +/- 1%, median within T +/- 0.5 ms, at least 99% of
// intervals at most 1.5 T, at most 0.2% above 2.5 T, under 1% in the 15.0-18.5 ms band.

export interface PacingStats {
  count: number;
  mean: number;
  median: number;
  p95: number;
  p99: number;
  /** Share (0..1) of intervals strictly above 1.5 T. */
  over1_5T: number;
  /** Share (0..1) of intervals strictly above 2.5 T. */
  over2_5T: number;
  /** Share (0..1) of intervals in the inclusive 15.0-18.5 ms band (a 60 Hz cadence). */
  band60: number;
}

export interface PacingVerdict {
  mean: boolean;
  median: boolean;
  within1_5T: boolean;
  over2_5T: boolean;
  /** null when the display itself runs inside the 60 Hz band, where the check means nothing. */
  band60: boolean | null;
  all: boolean;
}

export const BAND60_LOW_MS = 15.0;
export const BAND60_HIGH_MS = 18.5;

export function frameBudget(hz: number): number {
  return 1000 / hz;
}

/** Nearest-rank percentile of an ascending array. */
function percentile(sorted: readonly number[], p: number): number {
  if (sorted.length === 0) return Number.NaN;
  const rank = Math.max(1, Math.ceil((p / 100) * sorted.length));
  return sorted[Math.min(rank, sorted.length) - 1];
}

export function stats(deltas: readonly number[], T: number): PacingStats {
  const count = deltas.length;
  if (count === 0) {
    const nan = Number.NaN;
    return { count, mean: nan, median: nan, p95: nan, p99: nan, over1_5T: nan, over2_5T: nan, band60: nan };
  }
  const sorted = [...deltas].sort((a, b) => a - b);
  let sum = 0;
  let over15 = 0;
  let over25 = 0;
  let band = 0;
  for (const d of sorted) {
    sum += d;
    if (d > 1.5 * T) over15++;
    if (d > 2.5 * T) over25++;
    if (d >= BAND60_LOW_MS && d <= BAND60_HIGH_MS) band++;
  }
  const middle = count >> 1;
  const median = count % 2 ? sorted[middle] : (sorted[middle - 1] + sorted[middle]) / 2;
  return {
    count,
    mean: sum / count,
    median,
    p95: percentile(sorted, 95),
    p99: percentile(sorted, 99),
    over1_5T: over15 / count,
    over2_5T: over25 / count,
    band60: band / count,
  };
}

// Small tolerance so a series exactly on a limit is judged by the limit, not by
// floating-point noise in the sum.
const EPS = 1e-9;

export function passes(s: PacingStats, T: number): PacingVerdict {
  if (!(s.count > 0)) {
    return { mean: false, median: false, within1_5T: false, over2_5T: false, band60: false, all: false };
  }
  const mean = Math.abs(s.mean - T) <= 0.01 * T + EPS;
  const median = Math.abs(s.median - T) <= 0.5 + EPS;
  const within1_5T = s.over1_5T <= 0.01 + EPS;
  const over2_5T = s.over2_5T <= 0.002 + EPS;
  const band60 = T >= BAND60_LOW_MS && T <= BAND60_HIGH_MS ? null : s.band60 < 0.01;
  return { mean, median, within1_5T, over2_5T, band60, all: mean && median && within1_5T && over2_5T && band60 !== false };
}

function failures(v: PacingVerdict): number {
  return [v.mean, v.median, v.within1_5T, v.over2_5T, v.band60].filter((ok) => ok === false).length;
}

/**
 * Index of the worst run: the most failed criteria, then the larger shares of
 * very slow and slow frames, the 60 Hz band, the p99 and the mean's distance from T.
 */
export function worstRun(runs: readonly PacingStats[], T: number): number {
  const key = (s: PacingStats) => [
    failures(passes(s, T)),
    s.over2_5T,
    s.over1_5T,
    s.band60,
    s.p99,
    Math.abs(s.mean - T),
  ];
  let worst = 0;
  for (let i = 1; i < runs.length; i++) {
    const a = key(runs[i]);
    const b = key(runs[worst]);
    for (let k = 0; k < a.length; k++) {
      const x = Number.isNaN(a[k]) ? Infinity : a[k];
      const y = Number.isNaN(b[k]) ? Infinity : b[k];
      if (x !== y) {
        if (x > y) worst = i;
        break;
      }
    }
  }
  return worst;
}

export interface FrameClock {
  raf?: (callback: FrameRequestCallback) => number;
  now?: () => number;
  /** Called once per frame before the time is read, for scenarios that drive work each frame. */
  onFrame?: (time: number, frame: number) => void;
}

/**
 * Records the intervals between consecutive requestAnimationFrame callbacks for
 * `ms` milliseconds, timed with performance.now() inside each callback.
 */
export function recordFrames(ms: number, clock: FrameClock = {}): Promise<number[]> {
  const raf = clock.raf ?? ((callback: FrameRequestCallback) => requestAnimationFrame(callback));
  const now = clock.now ?? (() => performance.now());
  return new Promise((resolve) => {
    const deltas: number[] = [];
    let start = -1;
    let last = -1;
    let frame = 0;
    const tick = () => {
      const t = now();
      clock.onFrame?.(t, frame++);
      if (start < 0) {
        start = t;
      } else {
        deltas.push(t - last);
      }
      last = t;
      if (t - start >= ms) resolve(deltas);
      else raf(tick);
    };
    raf(tick);
  });
}
