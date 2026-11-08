import { Show, createSignal, onCleanup, onMount, type JSX } from "solid-js";
import { tr } from "../i18n";

// Task #247: Qt's StartupNotice (StartupNotice.cpp): a centred splash that
// appears only when startup takes longer than its delay, naming the hgs
// command-line step while the Windows install runs. The webview can be ready
// before the bridge, so the splash covers that wait.

/** Qt's StartupNotice delay before the splash appears. */
export const SPLASH_DELAY_MS = 1500;

export function splashTitle(): string {
  return tr("Starting Zerus…");
}

export function splashInstallLine(): string {
  return tr("Updating the hgs command line for local sessions");
}

export function BootSplash(props: { mode: () => "tauri" | "replay" | "starting" }): JSX.Element {
  const [late, setLate] = createSignal(false);
  onMount(() => {
    const timer = setTimeout(() => setLate(true), SPLASH_DELAY_MS);
    onCleanup(() => clearTimeout(timer));
  });
  return (
    <Show when={late() && props.mode() === "starting"}>
      <div class="boot-splash" role="status" aria-live="polite">
        <div class="boot-splash-box">
          <b>{splashTitle()}</b>
          <span>{splashInstallLine()}</span>
        </div>
      </div>
    </Show>
  );
}
