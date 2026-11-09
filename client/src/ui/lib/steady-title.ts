import { createSignal, untrack } from "solid-js";

/**
 * A title that holds still while the pointer is over its element
 * (zerus-5ryrx). WebKitGTK restarts a native tooltip whose text changes, so a
 * title fed by a ticking clock or a live row blinked under the pointer. Its
 * text is read when the pointer enters, as Qt computed a tooltip on hover,
 * and follows the live text again once the pointer leaves.
 */
export function steadyTitle(text: () => string | undefined) {
  const [held, setHeld] = createSignal<string | undefined>();
  const [over, setOver] = createSignal(false);
  return {
    title: () => (over() ? held() : text()),
    enter: () => { setHeld(untrack(text)); setOver(true); },
    leave: () => { setOver(false); },
  };
}
