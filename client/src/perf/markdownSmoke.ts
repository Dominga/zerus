import { invoke } from "@tauri-apps/api/core";
import { catalog } from "../store/catalog";
import { activity } from "../store/session";
import { setDetailTab, setPage, setSearch, setSelectedKey, setStateFilter } from "../store/ui";
import { INLINE_TABLE, SCREENSHOT_TABLE, WIDE_TABLE } from "./fixtures/markdownTables";

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));
async function until(test: () => boolean, reason: string) {
  const deadline = performance.now() + 15_000;
  while (!test()) { if (performance.now() > deadline) throw Error(reason); await sleep(16); }
}
function require(ok: boolean, reason: string) { if (!ok) throw Error(reason); }

/** Requires the synthetic replay and explicit fake CLI; it never opens a link. */
export async function markdownSmoke(allowed: boolean) {
  if (!allowed) return;
  const log = (line: string) => invoke("perf_log", { line: "markdown-smoke: " + line });
  try {
    const key = "\nrun:run-review-1";
    await until(() => !!catalog.rows[key], "replay catalog did not arrive");
    setPage("sessions"); setStateFilter("all"); setSearch(""); setSelectedKey(key); setDetailTab("activity");
    await until(() => activity.target()?.id === key && activity.loaded(), "initial replay Activity did not arrive");
    activity.applyActivity([{ computer: "", key: "run:run-review-1", kind: "replace", payload: { events: [{
      seq: 1, type: "AgentMessage", at: 42,
      detail: ["Before the table.", SCREENSHOT_TABLE, "After the table.", INLINE_TABLE, WIDE_TABLE, "```md\n" + SCREENSHOT_TABLE + "\n```"].join("\n\n"),
    }] } }]);
    await until(() => document.querySelectorAll(".act-table").length === 3, "Activity did not render structured tables");
    await sleep(150);
    require(document.querySelectorAll(".act-table").length === 3, "a late replay page replaced the rendered fixture");
    const tables = [...document.querySelectorAll<HTMLTableElement>(".act-table")];
    const wrappers = [...document.querySelectorAll<HTMLElement>(".act-table-wrap")];
    require(tables[0].querySelectorAll("th").length === 2 && tables[0].querySelectorAll("tbody tr").length === 6, "Russian table lost headers or rows");
    require(tables[0].querySelector("td b")?.textContent === "Работает в dev", "bold cell text was lost");
    require(wrappers[0].scrollWidth <= wrappers[0].clientWidth + 1, "ordinary table escaped the message width");
    const last = tables[0].querySelector("tbody tr:last-child td:last-child")!;
    require(last.getBoundingClientRect().height > parseFloat(getComputedStyle(last).lineHeight) * 2, "the long last column did not wrap");
    require(wrappers[2].scrollWidth > wrappers[2].clientWidth + 1, "wide table has no horizontal scrolling");
    const align = ["left", "center", "right"];
    [...tables[1].querySelectorAll("th")].forEach((cell, index) => require(getComputedStyle(cell).textAlign === align[index], "header alignment changed"));
    [...tables[1].querySelectorAll("tbody tr:first-child td")].forEach((cell, index) => require(getComputedStyle(cell).textAlign === align[index], "cell alignment changed"));
    require(tables[1].querySelector("code")?.textContent === "a|b", "escaped code pipe was split into columns");
    require(!tables[1].querySelector("img,script,[onerror],a[href^='javascript:']"), "cell text became executable HTML");
    require(tables[1].querySelector("a")?.getAttribute("href") === "https://example.com/docs", "guarded external link was lost");
    require(tables[1].querySelectorAll("a")[1]?.getAttribute("href") === "docs/example.md", "guarded file link was lost");
    require(document.querySelector(".act-code")?.textContent === SCREENSHOT_TABLE, "a fenced table became live markup");
    const scroll = document.querySelector<HTMLElement>(".act-scroll")!;
    require(scroll.scrollWidth <= scroll.clientWidth + 1, "wide table expanded the whole Activity");
    const headerStyle = getComputedStyle(tables[0].querySelector("th")!), cellStyle = getComputedStyle(last);
    await log("STYLE " + JSON.stringify({ headerColor: headerStyle.color, headerFont: headerStyle.fontSize,
      divider: headerStyle.borderBottomColor, bodyFont: cellStyle.fontSize }));
    // Leave the owner's reported table on screen for the built-app capture.
    activity.applyActivity([{ computer: "", key: "run:run-review-1", kind: "replace", payload: { events: [{
      seq: 2, type: "AgentMessage", at: 42, detail: SCREENSHOT_TABLE,
    }] } }]);
    await until(() => document.querySelectorAll(".act-table").length === 1, "reported table capture did not settle");
    await sleep(150);
    const header = document.querySelector(".act-table th")!;
    require(header.getBoundingClientRect().top >= scroll.getBoundingClientRect().top, "the reported table header is outside the viewport");
    await log("PASS (Russian rows, wrapping, wide scrolling, alignment, bold/code/guarded links, escaped pipes, literal HTML and fences)");
    await sleep(500);
    await invoke("exit_app", { code: 0 });
  } catch (error) { await log("FAIL " + String(error) + (error instanceof Error ? "\n" + (error.stack ?? "") : "")); await invoke("exit_app", { code: 1 }); }
}
