import { invoke } from "@tauri-apps/api/core";
import { flag, settingsReady } from "../store/settings";
import { windowLayer } from "../store/windowLayer";
import { setPage } from "../store/ui";
import { setSection } from "../ui/screens/settings/sections";

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));
async function until(test: () => boolean, reason: string) {
  const deadline = performance.now() + 15_000;
  while (!test()) { if (performance.now() > deadline) throw Error(reason); await sleep(16); }
}
function require(ok: boolean, reason: string) { if (!ok) throw Error(reason); }

/** Opt-in synthetic replay only; runs on a private X server or through design-ac. */
export async function windowLayerSmoke(allowed: boolean) {
  if (!allowed) return;
  const stage = new URLSearchParams(location.search).get("window-layer-smoke");
  const log = (line: string) => invoke("perf_log", { line: `window-layer-smoke ${stage}: ${line}` });
  try {
    await settingsReady;
    await until(() => windowLayer().supported && !windowLayer().busy, "native layer was not ready");
    const pin = () => document.querySelector<HTMLButtonElement>(".rail-btn.pin")!;
    const on = (value: boolean) => windowLayer().on === value && flag("workspace/alwaysOnTop") === value && pin().getAttribute("aria-pressed") === String(value);
    const initial = stage === "restart";
    await until(() => on(initial), "saved state did not apply at startup");
    if (!initial) {
      pin().focus(); pin().click();
      await until(() => on(true), "Rail pin did not persist the native setting");
      require(document.activeElement === pin(), "Rail pin lost focus");
    }
    setSection("appearance"); setPage("settings");
    const checkbox = () => [...document.querySelectorAll(".set-check label")].find((label) => label.textContent === "Keep Zerus above other windows")?.querySelector<HTMLInputElement>("input");
    await until(() => !!checkbox(), "Appearance did not open");
    const check = checkbox()!;
    require(check.checked && !check.disabled, "Settings and Rail disagree");
    check.focus();
    const hint = check.closest(".set-row")!.querySelector(".set-hint")!;
    const before = hint.getBoundingClientRect();
    check.click();
    require(document.activeElement === check, "Settings lost focus during the request");
    await until(() => on(false) && !check.checked, "Settings did not clear the Rail pin");
    const after = hint.getBoundingClientRect();
    require(before.height === after.height && before.width === after.width, "Keep Above hint changed layout");
    if (!initial) {
      pin().click(); await until(() => on(true) && check.checked, "Rail did not update Settings");
    }
    await log("PASS (startup, shared controls, focus, layout and persistence)");
    await invoke("exit_app", { code: 0 });
  } catch (error) {
    await log(`FAIL ${String(error)}`);
    await invoke("exit_app", { code: 1 });
  }
}
