import { lazy, type Component } from "solid-js";

// Rarely opened views live outside the entry chunk (zerus-ycycu): each loads on
// first use, and once the window has loaded and is idle they are fetched one
// at a time, so a first open shows no delay. Render each inside its own
// <Suspense>, so a view still loading never suspends the page around it.

type Loader = () => Promise<unknown>;
const pending: Loader[] = [];
let started = false;

/** How long after the window's load event idle preloading starts. */
export const PRELOAD_AFTER_LOAD_MS = 1500;

const idle = (run: () => void) => {
  const w = window as Window & { requestIdleCallback?: (run: () => void, options?: { timeout: number }) => number };
  if (w.requestIdleCallback) w.requestIdleCallback(run, { timeout: 5000 });
  else setTimeout(run, 200);
};

/**
 * Loads the registered views one at a time, each when `wait` calls back (the
 * window's idle callback in the app); a failed load is left to first use.
 */
export function preloadQueued(wait: (run: () => void) => void = idle): void {
  const step = () => {
    const load = pending.shift();
    if (!load) return;
    void load().catch(() => {}).finally(() => wait(step));
  };
  wait(step);
}

function start() {
  if (started || typeof window === "undefined" || import.meta.env.MODE === "test") return;
  started = true;
  const begin = () => setTimeout(() => preloadQueued(), PRELOAD_AFTER_LOAD_MS);
  if (document.readyState === "complete") begin();
  else window.addEventListener("load", begin, { once: true });
}

/**
 * `lazy(load)`, preloaded when the window is idle after it loaded. `pick`
 * chooses a named export; the module's default export is used without it.
 */
export function lazyView<P extends Record<string, unknown>, M>(load: () => Promise<M>, pick?: (module: M) => Component<P>): Component<P> & { preload: () => Promise<{ default: Component<P> }> } {
  // One load shared by the idle preload and first use; a failed one is
  // forgotten, so first use loads again instead of keeping the failure.
  let loading: Promise<{ default: Component<P> }> | undefined;
  const once = () => loading ??= load().then(
    (module) => ({ default: pick ? pick(module) : (module as { default: Component<P> }).default }),
    (error: unknown) => { loading = undefined; throw error; },
  );
  const view = lazy(once);
  pending.push(once);
  start();
  return view;
}
