import { createSignal } from "solid-js";

// Whether the window is hidden or minimised. The webview derives the page's
// visibility from its window; while it is hidden nothing is drawn, so the
// client pauses its motion (ui/motion.ts), its batch pump (bridge.ts) and the
// Working clocks (store/ui.ts), as the Qt widgets stop their timers in
// hideEvent.

export interface PageVisibility {
  hidden(): boolean;
  /** Calls `listener` after every change; returns the unsubscribe. */
  onChange(listener: () => void): () => void;
}

export const documentVisibility: PageVisibility = {
  hidden: () => typeof document !== "undefined" && document.visibilityState === "hidden",
  onChange(listener) {
    if (typeof document === "undefined") return () => {};
    document.addEventListener("visibilitychange", listener);
    return () => document.removeEventListener("visibilitychange", listener);
  },
};

const [hidden, setHidden] = createSignal(documentVisibility.hidden());
documentVisibility.onChange(() => setHidden(documentVisibility.hidden()));

/** Reactive: true while the window is hidden or minimised. */
export const pageHidden = hidden;
