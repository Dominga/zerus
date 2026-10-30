import { invoke } from "@tauri-apps/api/core";
import { updates } from "../store/updates";
import { page, setPage } from "../store/ui";
import { applyBatch, catalog } from "../store/catalog";

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

async function statusbarSmoke() {
  applyBatch({
    upserts: [], removes: [], replaces: [], activity: [],
    computers: [
      { computer: "", status: "live", attempt: 0 },
      { computer: "build-01", status: "live", attempt: 0 },
      { computer: "build-02", status: "reconnecting", attempt: 2 },
      { computer: "win-lab", status: "live", attempt: 0 },
      { computer: "mac", status: "live", attempt: 0 },
    ],
  });
  await until(() => !!document.querySelector(".vrow .rowhit"), "the replay session rows did not render");
  document.querySelector<HTMLElement>(".vrow .rowhit")!.click();
  await until(() => !!document.querySelector(".usage-refresh"), "the selected session's usage refresh did not render");
  await until(() => document.querySelector(".statusbar")?.textContent?.includes("4 of 5 machines connected, 1 offline") ?? false, "connection counts did not render");
  require(Object.keys(catalog.computers).length === 5, "the status fixture has extra computers");
  const retry = [...document.querySelectorAll<HTMLButtonElement>(".statusbar button")].find((button) => button.textContent === "Retry now");
  require(!!retry, "Retry now is missing in the connection-error state");
  retry!.click();

  const gap = document.querySelector(".statusbar .grow")!;
  for (const selector of [".status-update", ".status-restart", ".statusbar .warn-chip"]) {
    const button = document.querySelector<HTMLButtonElement>(selector)!;
    require(!!button, `missing status chip ${selector}`);
    require(button.classList.contains("chip-accent"), `${selector} is not an accent chip`);
    require(!button.querySelector(".zlogo"), `${selector} still has the spinning logo`);
    require((button.compareDocumentPosition(gap) & Node.DOCUMENT_POSITION_FOLLOWING) !== 0, `${selector} is not left of the gap`);
  }
  const fix = document.querySelector<HTMLButtonElement>(".statusbar .warn-chip")!;
  require(fix.textContent === "Fix login item", "the macOS login-item chip is missing");
  fix.click();
  await until(() => (document.getElementById("login-item-dialog") as HTMLDialogElement | null)?.open === true, "login-item confirmation did not open");
  const dialog = document.getElementById("login-item-dialog") as HTMLDialogElement;
  require(dialog.textContent?.includes("Update Only applies this at your next login") === true, "login-item confirmation is incomplete");
  [...dialog.querySelectorAll<HTMLButtonElement>("button")].find((button) => button.textContent === "Cancel")!.click();
  await until(() => !dialog.open, "login-item confirmation did not close");

  const refresh = document.querySelector<HTMLButtonElement>(".usage-refresh")!;
  require(refresh.title === "Refresh usage" && refresh.getAttribute("aria-label") === "Refresh usage", "usage refresh title is wrong");
  const icon = refresh.querySelector("svg")!;
  require(icon.getAttribute("width") === "12" && icon.getAttribute("height") === "12", "usage refresh is not 12 px");
  require(document.querySelector(".rail-new")?.getAttribute("data-tip") === "New session (Ctrl+N)", "New session shortcut is missing");
  require(document.querySelector('[aria-label="Sessions"]')?.getAttribute("data-tip") === "Sessions (Ctrl+2)", "Sessions shortcut is missing");
  const tips = [...document.querySelectorAll(".rail [data-tip]")].map((el) => el.getAttribute("data-tip"));
  require(tips.includes("Settings (Ctrl+,)"), "Settings shortcut is missing");
  require(tips.some((tip) => tip?.startsWith("Home — Zerus Next ") && tip.endsWith(" (Ctrl+1)")), "brand tooltip is missing its shortcut");
  const version = document.querySelector<HTMLButtonElement>(".rail-version")!;
  require(version.getAttribute("data-tip")?.endsWith(". Open About") === true, "version tooltip does not open About");
  version.click();
  await until(() => page() === "settings" && !!document.querySelector("#set-nav-about"), "the version button did not open About");
  setPage("sessions");
  await until(() => !!document.querySelector(".sessions-page"), "Sessions did not return after About");
}

export async function updatesSmoke(allowed: boolean) {
  const exit = (code: number) => invoke("exit_app", { code }).catch(() => {});
  const state = new URLSearchParams(location.search).get("updates-smoke") ?? "";
  if (!allowed) {
    log("updates-smoke not started: it needs ZERUS_CLIENT_REPLAY and ZERUS_CLIENT_PERF");
    return;
  }
  try {
    log(`updates-smoke location: ${location.href}`);
    await until(() => {
      const view = updates();
      if (state === "unchecked") return view.heading === "Updates have not been checked" && !view.release_enabled;
      if (state === "checking") return view.busy && view.heading === "Checking for updates…";
      if (state === "available") return view.available && !view.restart_required;
      if (state === "cached-error") return view.cached && !!view.error;
      if (state === "restart-required") return view.restart_required;
      if (state === "statusbar") return view.available && view.restart_required && view.login_item_deprioritized;
      return false;
    }, `update state ${state} did not arrive: ${JSON.stringify(updates())}`);
    if (state !== "statusbar") await until(() => !!document.querySelector(".updates-card"), "Updates did not render");
    const text = () => document.querySelector(".set-content")?.textContent ?? "";
    const updateButton = () => document.querySelector<HTMLButtonElement>(".status-update");
    const restartButton = () => document.querySelector<HTMLButtonElement>(".status-restart");
    const checkButton = () => [...document.querySelectorAll<HTMLButtonElement>(".set-content button")].find((button) => button.textContent === "Check now");

    if (state === "statusbar") {
      await statusbarSmoke();
    } else if (state === "unchecked") {
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
      const gap = document.querySelector(".statusbar .grow")!;
      for (const button of [updateButton()!, restartButton()!]) {
        require(button.classList.contains("chip-accent"), "update action is not an accent chip");
        require(!button.querySelector(".zlogo"), "update action still has the spinning logo");
        require((button.compareDocumentPosition(gap) & Node.DOCUMENT_POSITION_FOLLOWING) !== 0, "update action is not left of the gap");
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
