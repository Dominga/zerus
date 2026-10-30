/** Private built-app walk of the approved Project color popup. */
import { invoke } from "@tauri-apps/api/core";
import { setPage } from "../store/ui";
import { grouping } from "../store/organization";
import { settingsReady, settings } from "../store/settings";
import { projectsPage } from "../store/projects";
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
async function until(test: () => unknown, reason: string) { const deadline = performance.now() + 20_000; while (!test()) { if (performance.now() > deadline) throw Error(reason); await sleep(20); } }
function require(ok: unknown, reason: string) { if (!ok) throw Error(reason); }
export async function projectVividSmoke(allowed: boolean, restored: boolean) {
  if (!allowed) return;
  const log = (line: string) => invoke("perf_log", { line: "project-vivid-smoke: " + line });
  const button = (root: string, text: string) => [...document.querySelectorAll<HTMLButtonElement>(root + " button")].find((b) => b.textContent?.trim() === text);
  const style = () => document.querySelector<HTMLSelectElement>("#pc-style")!;
  const choose = (value: string) => { style().value = value; style().dispatchEvent(new Event("change", { bubbles: true })); };
  const open = async () => { button(".proj-right", "Edit color…")!.click(); await until(() => document.querySelector("dialog.project-color[open]"), "color popup missing"); };
  try {
    await settingsReady; setPage("projects");
    await until(() => grouping().ready && projectsPage().projects.some((p) => p.id === "p-sample-project") && button(".proj-right", "Edit color…") && !button(".proj-right", "Edit color…")!.disabled, "project consumer did not load");
    await open();
    require(style().value === "soft", "fresh/restored explicit false must show Soft fill");
    require(!document.querySelector(".pc-preview.vivid"), "soft preview has a bright fill");
    if (restored) { await log("RESTORED"); await sleep(500); await log("ESCAPE"); await until(() => !document.querySelector("dialog.project-color[open]"), "Escape did not close"); await until(() => document.activeElement === button(".proj-right", "Edit color…"), "focus did not return"); }
    else {
      await log("SOFT"); await sleep(500);
      choose("bright"); require(document.querySelector(".pc-preview.vivid"), "bright preview absent");
      require(settings["projects/vivid/p-sample-project"] !== true, "preview saved before Apply");
      await log("BRIGHT"); await sleep(500);
      button(".project-color", "Apply")!.click();
      await until(() => !document.querySelector("dialog.project-color[open]") && !!document.querySelector(".proj-preview.vivid"), "Apply did not store/display local vivid");
      require(settings["projects/vivid/p-sample-project"] === true, "exact ID preference missing");
      await open(); choose("soft"); button(".project-color", "Cancel")!.click();
      require(settings["projects/vivid/p-sample-project"] === true && !!document.querySelector(".proj-preview.vivid"), "Cancel saved a preview");
      await open(); choose("soft");
      const input = document.querySelector<HTMLInputElement>("#pc-hex")!; input.value = "#invalid"; input.dispatchEvent(new Event("input", { bubbles: true }));
      require(button(".project-color", "Apply")!.disabled, "invalid color enabled Apply");
      document.querySelector<HTMLButtonElement>(".pc-sw")!.click();
      button(".project-color", "Apply")!.click();
      await until(() => !document.querySelector("dialog.project-color[open]") && settings["projects/vivid/p-sample-project"] === false, "shared color/local soft apply did not settle");
      require(!document.querySelector(".proj-preview.vivid"), "explicit false was lost");
      await until(() => projectsPage().projects.find((p) => p.id === "p-sample-project")?.color === "#ffcf33", "shared color did not use the existing organization writer");
      await log("SAVED"); await sleep(500);
    }
    await log("PASS"); await invoke("exit_app", { code: 0 });
  } catch (error) { await log("FAIL " + String(error)); await invoke("exit_app", { code: 1 }); }
}
