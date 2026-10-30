/** Explicit SSH click against a private replay and a recording terminal. */
import { invoke } from "@tauri-apps/api/core";
import { catalog } from "../store/catalog";
import { setDetailTab, setPage, setSelectedKey } from "../store/ui";
import { closeSessionFile, openSessionFile, sessionFile } from "../ui/sessions/sessionFiles";
const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));
async function until(test: () => boolean, reason: string) {
  const deadline = performance.now() + 15_000;
  while (!test()) { if (performance.now() > deadline) throw Error(reason); await sleep(20); }
}
export async function fileSshSmoke(allowed: boolean) {
  if (!allowed) return;
  const log = (line: string) => invoke("perf_log", { line: "file-ssh-smoke: " + line });
  const button = () => [...document.querySelectorAll<HTMLButtonElement>(".session-file button")].find((b) => b.textContent?.trim() === "Open SSH terminal");
  try {
    await until(() => Object.values(catalog.rows).some((s) => s.computer === "example-peer"), "private peer missing");
    const session = Object.values(catalog.rows).find((s) => s.computer === "example-peer")!;
    setPage("sessions"); setSelectedKey(session.key); setDetailTab("activity");
    await until(() => !!document.querySelector(".act-scroll"), "Activity not rendered");
    const focus = document.querySelector<HTMLButtonElement>(".rail button")!; focus.focus();
    async function show() {
      openSessionFile(session, "/srv/example/report.md:19");
      await until(() => !!sessionFile() && !sessionFile()!.checking && !!button(), "file dialog not ready");
      if (button()!.disabled) throw Error("Qt SSH action disabled");
    }
    await show(); await log("INITIAL"); await sleep(700);
    button()!.click(); await until(() => !sessionFile(), "successful SSH click did not close the dialog");
    if (document.activeElement !== focus) throw Error("SSH success did not restore focus");
    await log("OPENED");
    await show(); button()!.click();
    await until(() => !!sessionFile()?.error, "launcher failure missing");
    if (!sessionFile()!.error.startsWith("Could not open the SSH terminal: ")) throw Error("launcher error differs from Qt mapping");
    await log("FAILED"); await sleep(500); await log("ESCAPE_FAILURE");
    await until(() => !sessionFile(), "Escape did not close failed dialog");
    if (document.activeElement !== focus) throw Error("failed SSH Escape did not restore focus");
    await show(); button()!.click();
    await until(() => !!sessionFile()?.error, "forgotten alias not refused");
    if (sessionFile()!.error !== "Could not open the SSH terminal: The session state changed. Open its actions again.") throw Error("forgotten alias was not refused exactly");
    await log("REFUSED"); await log("ESCAPE_REFUSED");
    await until(() => !sessionFile(), "Escape did not close refused dialog");
    await log("PASS"); await invoke("exit_app", { code: 0 });
  } catch (error) {
    await log("FAIL " + String(error)); closeSessionFile(); await invoke("exit_app", { code: 1 });
  }
}
