import { createSignal, onCleanup } from "solid-js";

/** Applies only the answer to the latest request, so a slow older pull never overwrites a newer view. */
export function latest<T>(set: (value: T) => void) {
  let sequence = 0;
  return async (request: () => Promise<T>) => {
    const mine = ++sequence;
    const value = await request();
    if (mine === sequence) set(value);
  };
}

/** The clock in Unix seconds, refreshed every `ms` and stopped with the owner; for time labels only. */
export function useNow(ms: number): () => number {
  const [now, setNow] = createSignal(Date.now() / 1000);
  const timer = setInterval(() => setNow(Date.now() / 1000), ms);
  onCleanup(() => clearInterval(timer));
  return now;
}
