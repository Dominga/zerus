import { createSignal } from "solid-js";

export interface PromptSuggestion { text: string; at: number }

/** Qt's per-session suggestion and dismissal maps, bounded for the catalog's lifetime. */
export function createSuggestions(limit = 50_000) {
  const [current, setCurrent] = createSignal(new Map<string, PromptSuggestion>());
  const dismissed = new Map<string, number>();
  const bound = (map: Map<string, unknown>) => {
    while (map.size > limit) map.delete(map.keys().next().value!);
  };
  return {
    set(key: string, text: string, at: number) {
      if (!key) return;
      if (!text) {
        const next = new Map(current());
        next.delete(key);
        dismissed.delete(key);
        setCurrent(next);
        return;
      }
      if (dismissed.get(key) === at || current().get(key)?.text === text && current().get(key)?.at === at) return;
      const next = new Map(current());
      next.set(key, { text, at });
      bound(next);
      setCurrent(next);
    },
    get(key: string, waiting = false): string {
      return waiting ? "" : current().get(key)?.text ?? "";
    },
    dismiss(key: string) {
      const item = current().get(key);
      if (!item) return;
      dismissed.set(key, item.at);
      bound(dismissed);
      const next = new Map(current());
      next.delete(key);
      setCurrent(next);
    },
  };
}

export const suggestions = createSuggestions();

/** Tab accepts only an unlocked, completely empty field; every other Tab keeps its focus behavior. */
export function suggestedInput(text: string, suggestion: string, locked: boolean): string | undefined {
  return !locked && text === "" && suggestion ? suggestion : undefined;
}
