/** Private built-app walk: an isolated replay and explicit fake hgs only. */
import { invoke } from "@tauri-apps/api/core";
import { catalog } from "../store/catalog";
import { setSelectedKey } from "../store/ui";
import { openInspector } from "../store/inspector";
import { lastCopied } from "../store/actions";
import { newSession } from "../store/newSession";
import { worktreeReview, worktreeReviewDialog, worktreeReviewLoading } from "../store/worktreeCleanup";
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
function require(ok: unknown, why: string) { if (!ok) throw Error(why); }
async function until(test: () => unknown, why: string) { const deadline = performance.now() + 20_000; while (!test()) { if (performance.now() > deadline) throw Error(why); await sleep(20); } }
export async function worktreeReviewSmoke(allowed: boolean) {
  if (!allowed) return;
  const log = (line: string) => invoke("perf_log", { line: "worktree-review-smoke: " + line });
  const button = (root: string, label: string) => [...document.querySelectorAll<HTMLButtonElement>(root + " button")].find((b) => b.textContent?.trim() === label);
  const menu = async (path: string) => { const card = [...document.querySelectorAll<HTMLElement>(".icard.wt")].find((c) => c.querySelector(".wt-path")?.textContent?.includes(path))!; require(card, "missing checkout card " + path); const r = card.getBoundingClientRect(); card.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, clientX: r.left + 50, clientY: r.top + 30 })); await until(() => document.querySelector(".menu[role=menu]"), "context menu missing"); };
  const choose = (names: string[]) => { for (const row of document.querySelectorAll<HTMLTableRowElement>(".wc-table tbody tr")) { const input = row.querySelector<HTMLInputElement>("input")!; const wanted = names.some((n) => row.title.endsWith("/" + n)); if (!input.disabled && input.checked !== wanted) input.click(); } };
  try {
    await until(() => Object.values(catalog.rows).some((s) => s.name === "codex/example/probe"), "synthetic session missing");
    const s = Object.values(catalog.rows).find((s) => s.name === "codex/example/probe")!;
    setSelectedKey(s.key); openInspector("worktrees");
    await until(() => document.querySelectorAll(".icard.wt").length >= 4 && !button(".inspector", "Clean up…")?.disabled, "worktree panel did not load");
    await log("PANEL"); await sleep(500);
    await menu("/ignored"); button(".menu", "Copy path")!.click(); await until(() => lastCopied()?.endsWith("/ignored"), "Copy path failed");
    button(".inspector", "New session…")!.click(); await until(() => newSession()?.folderPath.endsWith("/ignored"), "New session did not retain selected checkout");
    require(!newSession()?.submitted, "opening New session created work"); document.querySelector<HTMLButtonElement>(".ns-dlg button[aria-label=Close]")!.click();
    await menu("/missing"); button(".menu", "Forget missing worktree…")!.click();
    await until(() => worktreeReview()?.focus.endsWith("/missing"), "missing worktree focus lost"); require(worktreeReview()?.rows.length === 1, "focused review escaped its checkout");
    button(".worktree-review", "Close")!.click(); await until(() => !worktreeReviewDialog(), "review did not close");
    await until(() => !button(".inspector", "Clean up…")!.disabled, "catalog did not refresh after review");
    button(".inspector", "Clean up…")!.click(); await until(() => worktreeReview()?.rows.length === 6, "full review missing");
    require(!document.querySelector<HTMLInputElement>(".worktree-review .check input")!.checked, "branch opt-in must default off");
    const blocked = [...document.querySelectorAll<HTMLTableRowElement>(".wc-table tbody tr")].find((r) => r.title.endsWith("/blocked"))!; require(blocked.querySelector<HTMLInputElement>("input")!.disabled, "known use offered removal");
    await log("REVIEW"); await sleep(500); choose(["ignored"]);
    button(".worktree-review", "Remove selected…")!.click(); await until(() => document.querySelector("#worktree-confirm-title"), "confirmation absent");
    require(document.querySelector<HTMLButtonElement>("dialog:has(#worktree-confirm-title) .solid-danger")!.disabled, "local files deleted without consent");
    require(document.querySelector("dialog:has(#worktree-confirm-title)")!.textContent!.includes("target/"), "ignored summary missing top paths");
    await log("CONFIRMATION"); await sleep(500); await log("ESCAPE_CONFIRM"); await until(() => !document.querySelector("#worktree-confirm-title") && !worktreeReviewLoading(), "Escape did not cancel confirmation");
    require(worktreeReview()?.rows.every((r) => !["Removed", "Unknown"].includes(r.state)), "Cancel had an effect");
    choose(["clean", "ignored", "missing", "unknown", "changed"]);
    document.querySelector<HTMLInputElement>(".worktree-review .check input")!.click();
    button(".worktree-review", "Remove 5 worktrees…")!.click(); await until(() => document.querySelector("#worktree-confirm-title"), "batch confirmation absent");
    const c = document.querySelector<HTMLDialogElement>("dialog:has(#worktree-confirm-title)")!;
    require(c.textContent!.includes("Merged branches deleted: clean, ignored, changed."), "confirmation did not name exact merged branches");
    c.querySelector<HTMLInputElement>(".check input")!.click(); c.querySelector<HTMLButtonElement>(".solid-danger")!.click();
    await until(() => worktreeReview()?.busy, "removal did not begin"); require(button(".worktree-review", "Close")!.disabled, "cleanup can be abandoned during removal");
    await log("REMOVING"); await sleep(500); await until(() => worktreeReview() && !worktreeReview()!.busy && worktreeReview()!.unknown, "outcomes did not settle");
    require(worktreeReview()!.rows.filter((r) => r.state === "Removed" || r.state === "Forgotten").length === 3, "success outcomes lost");
    require(worktreeReview()!.rows.some((r) => r.path.endsWith("/changed") && r.state === "Kept"), "stale fingerprint was not refused");
    await log("OUTCOMES"); await sleep(500); button(".worktree-review", "Review again")!.click();
    await until(() => worktreeReview()!.rows.some((r) => r.path.endsWith("/changed") && r.selectable), "verified refusal did not return to review");
    require(worktreeReview()!.rows.some((r) => r.path.endsWith("/unknown") && !r.selectable && r.state === "Unknown"), "unknown was rearmed by refresh");
    require(!document.querySelector<HTMLInputElement>(".worktree-review .check input")!.checked, "branch consent was remembered");
    await log("ESCAPE_REVIEW"); await until(() => !worktreeReviewDialog(), "Escape did not close the review");
    await until(() => document.activeElement?.textContent?.trim() === "Clean up…", "review focus did not return");
    await until(() => !button(".inspector", "Clean up…")!.disabled, "catalog not refreshed"); await menu("/unknown"); button(".menu", "Remove worktree…")!.click();
    await until(() => worktreeReview()?.rows.length === 1 && worktreeReview()?.unknown, "unknown outcome not retained after reopening");
    require(button(".worktree-review", "Remove selected…")!.disabled && button(".worktree-review", "Review again")!.disabled, "unknown offered a retry");
    await log("UNKNOWN_REOPEN"); await sleep(500); button(".worktree-review", "Close")!.click();
    await log("PASS"); await invoke("exit_app", { code: 0 });
  } catch (error) { await log("FAIL " + String(error) + (error instanceof Error ? "\n" + error.stack : "")); await log("STATE " + JSON.stringify(worktreeReview())); await invoke("exit_app", { code: 1 }); }
}
