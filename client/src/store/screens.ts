import { createEffect, createRoot, createSignal, on, type Accessor } from "solid-js";
import { onScreensChanged, screenVisible } from "../bridge";
import type { Screen } from "../types";
import type { PageVisibility } from "./visibility";
import type { Page } from "./ui";
import { loadAccounts } from "./accounts";
import { loadMachines } from "./machines";
import { loadProjects } from "./projects";

// The screens hub runs a screen's reads only while it shows: this keeps
// `screen_visible` in step with the page and the window, and pulls a view
// again when the hub reports it changed.

const SCREEN: Partial<Record<Page, Screen>> = { dashboard: "overview", projects: "projects", machines: "machines", accounts: "accounts" };

/** The screen whose reads run: the page's, while the window shows. */
export const screenOf = (page: Page, hidden: boolean): Screen | undefined => (hidden ? undefined : SCREEN[page]);

/** Pulls a screen's view again; Overview and Accounts share the accounts view. */
export function reload(screen: Screen): Promise<void> {
  if (screen === "overview" || screen === "accounts") return loadAccounts();
  if (screen === "machines") return loadMachines();
  return loadProjects();
}

/** Starts the wiring; returns its stop. */
export function startScreens(page: Accessor<Page>, visibility: PageVisibility): () => void {
  return createRoot((dispose) => {
    const [hidden, setHidden] = createSignal(visibility.hidden());
    const stopVisibility = visibility.onChange(() => setHidden(visibility.hidden()));
    const stopEvents = onScreensChanged((screen) => {
      if (screen === screenOf(page(), hidden())) void reload(screen);
    });
    createEffect(on(() => screenOf(page(), hidden()), (next, previous) => {
      if (previous) void screenVisible(previous, false);
      if (next) void screenVisible(next, true).then(() => reload(next));
    }));
    return () => {
      stopVisibility();
      stopEvents();
      dispose();
    };
  });
}
