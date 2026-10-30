/** Built-app contract walk. Requires replay plus an explicitly selected fake hgs. */
import { invoke } from "@tauri-apps/api/core";
import { catalog } from "../store/catalog";
import { setDetailTab, setPage, setSelectedKey } from "../store/ui";
import { notices } from "../store/notices";
import { actOnSessionFile, closeSessionFile, downloadSessionFile, openSessionFile, sessionFile } from "../ui/sessions/sessionFiles";
import type { Session } from "../ui/sessions/types";
const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));
async function until(test: () => boolean, reason: string) {
  const deadline = performance.now() + 20_000;
  while (!test()) { if (performance.now() > deadline) throw Error(reason); await sleep(20); }
}
function require(ok: boolean, reason: string) { if (!ok) throw Error(reason); }
export async function fileReferenceSmoke(allowed: boolean) {
  if (!allowed) return;
  const log = (line: string) => invoke("perf_log", { line: "file-reference-smoke: " + line });
  const session = { computer: "example-peer", catalogKey: "run:synthetic-file", cwdPath: "/srv/example" } as Session;
  const button = (text: string) => [...document.querySelectorAll<HTMLButtonElement>(".session-file button")].find((b) => b.textContent?.trim() === text);
  async function show(path: string) {
    openSessionFile(session, path);
    await until(() => !!document.querySelector("dialog.session-file[open]") && !!sessionFile() && !sessionFile()!.checking, "file dialog/cache check did not settle");
  }
  try {
    await until(() => Object.keys(catalog.rows).length > 0, "replay catalog did not arrive");
    const replayKey = Object.keys(catalog.rows).find((key) => catalog.rows[key].name === "claude/sample-project/review")!;
    require(!!replayKey, "synthetic replay row is missing");
    setPage("sessions"); setSelectedKey(replayKey); setDetailTab("activity");
    await until(() => !!document.querySelector(".act-scroll"), "Activity did not render");
    const focus = document.querySelector<HTMLButtonElement>(".rail button")!; focus.focus();
    await show("/srv/example/save.md");
    require(!!button("Download…") && !!button("Download and open"), "approved download buttons are missing");
    require(!button("Open local copy") && !button("Show file"), "local actions appeared before a successful copy");
    await log("INITIAL"); await sleep(700);
    button("Download…")!.click(); await log("SAVE_DIALOG");
    await until(() => !!sessionFile()?.local_available && !sessionFile()?.downloading && !sessionFile()?.picking, "native Save As did not save the file");
    require(sessionFile()!.hint.startsWith("Saved on this computer:"), "saved text differs from Qt");
    require(!!button("Download again") && !!button("Open local copy") && !!button("Show file"), "saved-copy actions are missing");
    require(!button("Download again")!.classList.contains("primary"), "Download again retained the default-action style");
    await log("SAVED"); await sleep(700);
    button("Show file")!.click(); await until(() => !sessionFile(), "Show file did not close the modal");
    await log("SHOW");
    await show("/srv/example/open.md"); button("Download and open")!.click();
    await until(() => !sessionFile(), "Download and open did not close after completion"); await log("OPEN");
    await show("/srv/example/open.md"); require(sessionFile()!.local_available, "configured cache root was not reused");
    button("Open local copy")!.click(); await until(() => !sessionFile(), "Open local copy did not close"); await log("COPY");
    await show("/srv/example/failure.md"); button("Download and open")!.click();
    await until(() => !!sessionFile()?.error, "failed transfer did not report an error");
    require(sessionFile()!.error === "Download failed: Synthetic remote failure.", "failure text differs from Qt");
    require(!sessionFile()!.downloading && !button("Download…")!.disabled && !button("Download and open")!.disabled, "failed transfer left actions disabled");
    await log("FAILED"); await sleep(500);
    // A real Escape is injected by the private X11 driver, including native focus handling.
    await log("ESCAPE"); await until(() => !sessionFile(), "Escape did not dismiss the dialog");
    require(document.activeElement === focus, "Escape did not restore the originating control's focus"); await log("FOCUS");
    await show("/srv/example/slow.md"); downloadSessionFile(true);
    await until(() => !!sessionFile()?.downloading, "slow transfer did not begin"); closeSessionFile();
    await sleep(1400); require(!sessionFile(), "download completion reopened a closed dialog"); await log("CLOSED_COMPLETION");
    await show("/srv/example/failure.md"); downloadSessionFile(true); closeSessionFile();
    await until(() => notices().some((n) => n.text === "Download failed: Synthetic remote failure." && n.sticky), "closed failed download did not post its sticky notice");
    await log("PASS"); await sleep(300); await invoke("exit_app", { code: 0 });
  } catch (error) {
    await log("FAIL " + String(error) + (error instanceof Error ? "\n" + error.stack : ""));
    await log("STATE " + JSON.stringify({ view: sessionFile(), modal: !!document.querySelector("dialog.session-file[open]"), notices: notices() }));
    closeSessionFile(); await invoke("exit_app", { code: 1 });
  }
}
