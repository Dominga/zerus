import { createEffect, createSignal, on, onCleanup } from "solid-js";
import { ZERUS_PETAL } from "../lib/qt-icons";
import type { LogoMotion } from "../../store/ui";
import "../styles/logo.css";

// The rail's Zerus mark (tray/src/SwarmButton.h). While any session needs
// attention it takes the attention colour and turns; otherwise it rests in the
// neutral colour. The mark is one layer that only rotates (and, in the Qt
// variant, fades), so the compositor runs it at the display rate without
// repainting. Motion stops for reduced motion and while the page is hidden;
// when attention clears, the current turn finishes and the mark settles.

const PETALS = [0, 120, 240]
  .map((deg) => `<path transform="rotate(${deg} 50 50) translate(56 30) scale(.88) translate(-56 -30)" d="${ZERUS_PETAL}"/>`)
  .join("");
const MARK = `<g transform="translate(50 50) scale(1.06) translate(-50 -50)">${PETALS}</g>`;

export function ZerusLogo(props: { attention: boolean; motion: LogoMotion; size?: number }) {
  const [running, setRunning] = createSignal(props.attention);
  const [hidden, setHidden] = createSignal(document.visibilityState === "hidden");

  // Start at once; stop only at the end of a cycle, where the mark is at rest.
  const reduced = () => matchMedia("(prefers-reduced-motion: reduce)").matches || document.documentElement.dataset.reduceMotion === "1";
  createEffect(on(() => props.attention, (on) => {
    if (on) setRunning(true);
    else if (reduced() || hidden()) setRunning(false);
  }));
  const onIteration = (e: AnimationEvent) => {
    if (!props.attention && (e.target as Element).classList.contains("zl-clock")) setRunning(false);
  };
  const onVisibility = () => setHidden(document.visibilityState === "hidden");
  document.addEventListener("visibilitychange", onVisibility);
  onCleanup(() => document.removeEventListener("visibilitychange", onVisibility));

  return (
    <span class="zlogo" style={{ "--zl-size": `${props.size ?? 28}px` }}
      data-motion={props.motion} data-attention={props.attention ? "1" : "0"} data-running={running() ? "1" : "0"} data-paused={hidden() ? "1" : "0"}
      onAnimationIteration={onIteration} aria-hidden="true">
      <span class="zl-mark">
        <svg class="zl-fade" viewBox="0 0 100 100" fill="currentColor" innerHTML={MARK} />
      </span>
      {/* An invisible element carries the 3 s cycle so the stop always lands on a cycle boundary. */}
      <i class="zl-clock" />
    </span>
  );
}
