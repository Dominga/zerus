import { For, createEffect, createSignal, on, onCleanup } from "solid-js";
import { ZERUS_PETAL } from "../lib/qt-icons";
import type { LogoMotion } from "../state";
import "../styles/logo.css";

// The rail's Zerus mark (tray/src/SwarmButton.h). While any session needs
// attention it takes the attention colour and turns; otherwise it rests in the
// neutral colour. Each blade is its own HTML layer, so every variant animates
// only transform and opacity on the compositor and never repaints the window.
// Motion stops for reduced motion and while the page is hidden; when attention
// clears, the current turn finishes and the mark settles at rest.

const BLADES = [0, 120, 240];
const blade = (deg: number) =>
  `<g transform="translate(50 50) scale(1.06) translate(-50 -50)"><path transform="rotate(${deg} 50 50) translate(56 30) scale(.88) translate(-56 -30)" d="${ZERUS_PETAL}"/></g>`;

export function ZerusLogo(props: { attention: boolean; motion: LogoMotion; size?: number }) {
  const [running, setRunning] = createSignal(props.attention);
  const [hidden, setHidden] = createSignal(document.visibilityState === "hidden");

  // Start at once; stop only at the end of a turn, where every layer is at rest.
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
      <i class="zl-glow" />
      <i class="zl-ring" />
      <i class="zl-ring r2" />
      <span class="zl-mark">
        <span class="zl-fade">
          <For each={BLADES}>{(deg, i) => (
            <svg class={`zl-blade b${i()}`} viewBox="0 0 100 100" fill="currentColor" innerHTML={blade(deg)} />
          )}</For>
        </span>
      </span>
      {/* An invisible element carries the 3 s cycle so the stop always lands on a turn boundary. */}
      <i class="zl-clock" />
    </span>
  );
}
