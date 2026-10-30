import { invoke } from "@tauri-apps/api/core";
import { updates } from "../store/updates";
import { page } from "../store/ui";

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

async function until(ready: () => boolean, what: string): Promise<void> {
  const started = performance.now();
  while (!ready()) {
    if (performance.now() - started > 15_000) throw new Error(what);
    await sleep(16);
  }
}

function log(line: string) {
  void invoke("perf_log", { line }).catch(() => {});
}

function require(ok: boolean, what: string) {
  if (!ok) throw new Error(what);
}

export async function updatesSmoke(allowed: boolean) {
  const exit = (code: number) => invoke("exit_app", { code }).catch(() => {});
  const state = new URLSearchParams(location.search).get("updates-smoke") ?? "";
  if (!allowed) {
    log("updates-smoke not started: it needs ZERUS_CLIENT_REPLAY and ZERUS_CLIENT_PERF");
    return;
  }
  try {
    await until(() => {
      const view = updates();
      if (state === "unchecked") return view.heading === "Updates have not been checked" && !view.release_enabled;
      if (state === "checking") return view.busy && view.heading === "Checking for updates…";
      if (state === "available") return view.available && !view.restart_required;
      if (state === "cached-error") return view.cached && !!view.error;
      if (state === "restart-required") return view.restart_required;
      return false;
    }, `update state ${state} did not arrive`);
    await until(() => !!document.querySelector(".updates-card"), "Updates did not render");
    const text = () => document.querySelector(".set-content")?.textContent ?? "";
    const updateButton = () => document.querySelector<HTMLButtonElement>(".status-update");
    const restartButton = () => document.querySelector<HTMLButtonElement>(".status-restart");
    const checkButton = () => [...document.querySelectorAll<HTMLButtonElement>(".set-content button")].find((button) => button.textContent === "Check now");

    if (state === "unchecked") {
      require(text().includes("Not checked"), "unchecked Latest release is missing");
      require(checkButton()?.disabled === false, "unchecked Check now is disabled");
      require(!updateButton() && !restartButton(), "unchecked state shows a workspace update action");
    } else if (state === "checking") {
      require(checkButton()?.disabled === true, "checking Check now is enabled");
      require(!text().includes("A previous comparison"), "checking state shows its old comparison");
    } else if (state === "available") {
      require(text().includes("Update available") && text().includes("View release"), "available card is incomplete");
      require(text().includes("Download dev source"), "conditional download action is missing");
      require(updateButton()?.title === "Zerus 0.47.0 is available. Open Updates.", "New version tooltip differs from Qt");
      updateButton()!.click();
      await sleep(100);
      require(page() === "settings", "New version opened a session instead of Settings");
      const disclosure = [...document.querySelectorAll<HTMLButtonElement>(".set-content button")].find((button) => button.textContent === "Show installation details");
      disclosure!.click();
      await until(() => text().includes("yay -S zerus"), "installation command did not open");
      require(document.querySelector('[aria-label="Copy the update command"]') !== null, "Copy command accessible name is missing");
      [...document.querySelectorAll<HTMLButtonElement>(".set-content button")].find((button) => button.textContent === "Hide installation details")!.click();
      await until(() => !text().includes("yay -S zerus"), "installation details did not close");
    } else if (state === "cached-error") {
      require(text().includes("Showing the last verified catalog."), "cached notice is missing");
      require(text().includes("Check failed. Try again later."), "check error is missing");
    } else if (state === "restart-required") {
      require(!!updateButton() && !!restartButton(), "restart state does not show both workspace actions");
      require((updateButton()!.compareDocumentPosition(restartButton()!) & Node.DOCUMENT_POSITION_FOLLOWING) !== 0, "Restart is not after New version");
      require(restartButton()!.title === "Zerus was updated on disk. Drafts are kept; sessions keep running.", "Restart tooltip differs from Qt");
      for (const button of [updateButton()!, restartButton()!]) {
        const mark = button.querySelector<HTMLElement>(".zlogo");
        require(mark?.style.getPropertyValue("--zl-size") === "14px", "Zerus mark is not 14 px");
      }
    }
    log(`updates-smoke ${state}: ready`);
    await sleep(3000);
    await exit(0);
  } catch (error) {
    log(`updates-smoke ${state}: failed: ${String(error)}`);
    await exit(1);
  }
}
