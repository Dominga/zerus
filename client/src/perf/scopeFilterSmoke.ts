import { invoke } from "@tauri-apps/api/core";
import { applyBatch, catalog } from "../store/catalog";
import { filterAccounts } from "../store/accountFilters";
import { contentSearch, searchItems, searchStatus } from "../store/contentSearch";
import { filterCounts, listRows, pickerCounts } from "../ui/sessions/model";
import { accountFilter, folderFilter, selectedKey, setAccountFilter, setFolderFilter, setPage, setProjectFilter, setSearch, setSelectedKey, setStateFilter } from "../store/ui";
import { openInspector } from "../store/inspector";
import { emptyBatch } from "../types";
import { accountsPage } from "../store/accounts";
import { menu } from "../store/ui";
import { rowBinding } from "../ui/sessions/scope-filters";

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));
async function until(test: () => boolean, reason: string) {
  const deadline = performance.now() + 15_000;
  while (!test()) { if (performance.now() > deadline) throw Error(reason); await sleep(16); }
}
function require(ok: boolean, reason: string) { if (!ok) throw Error(reason); }
const buttons = (selector: string) => [...document.querySelectorAll<HTMLButtonElement>(selector)];
function clickMenu(label: string) {
  const button = buttons(".menu-item").find((b) => b.textContent?.trim().startsWith(label));
  if (!button) throw Error(`menu entry missing: ${label}`);
  button.click();
}
function typeSearch(value: string) {
  const input = document.querySelector<HTMLInputElement>(".sp-search input")!;
  input.value = value; input.dispatchEvent(new Event("input", { bubbles: true }));
}

/** Only the explicit fake CLI and synthetic replay can run this interaction test. */
export async function scopeFilterSmoke(allowed: boolean) {
  if (!allowed) return;
  const log = (line: string) => invoke("perf_log", { line: "scope-filter-smoke: " + line });
  try {
    await until(() => !!catalog.rows["\nrun:run-lint-1"], "replay catalog missing");
    setPage("sessions"); setStateFilter("all"); setSearch(""); setSelectedKey(undefined); setProjectFilter([]); setAccountFilter([]); setFolderFilter(undefined);
    const rows = ["work-a", "work-b", "personal"].map((id, i) => ({ computer: "", key: `run:${id}`, row: {
      name: `claude/example/${id}`, title: `Needle ${id}`, agent: "claude", machine: "workstation", state: "running", status: "ready",
      run_id: id, conversation_id: `conv-${id}`, account_provider: "claude", account_id: i < 2 ? "work" : "personal", account_home: `/srv/example/${i < 2 ? "work-profile" : "personal-profile"}`,
      cwd: i === 1 ? "/srv/example/other" : "/srv/example/app", cwd_canonical: i === 1 ? "/srv/example/other" : "/srv/example/app", git_root: i === 1 ? "/srv/example/other" : "/srv/example/app",
    } }));
    applyBatch({ ...emptyBatch(), replaces: [{ computer: "", rows }] });
    await until(() => listRows().filter((r) => r.type === "card").length === 3, "initial three rows missing");
    await sleep(300);
    const openPicker = (name: string) => {
      const button = document.querySelector<HTMLButtonElement>(`[aria-label='Filter sessions by ${name}']`)!, r = button.getBoundingClientRect();
      button.dispatchEvent(new MouseEvent("click", { bubbles: true, clientX: r.left + r.width / 2, clientY: r.top + r.height / 2 }));
    };
    openPicker("accounts");
    await until(() => filterAccounts().some((a) => a.label === "Work") && buttons(".menu-item").some((b) => b.textContent?.trim() === "Work (2)"), "account catalog did not update the open picker");
    require(buttons(".menu-item").some((b) => b.textContent?.trim() === "Personal (1)"), "personal count wrong");
    await sleep(200);
    const picker = document.querySelector<HTMLElement>(".scope-btn")!, item = document.querySelector<HTMLElement>(".menu-item")!;
    await log("STYLE " + JSON.stringify({ scopeHeight: picker.getBoundingClientRect().height, scopeFont: getComputedStyle(picker).fontSize, menuFont: getComputedStyle(item).fontSize, menuHeight: item.getBoundingClientRect().height, providerSize: document.querySelector(".menu-agent svg")?.getAttribute("width") }));
    await log("ACCOUNTS_OPEN"); await sleep(300);
    clickMenu("Work (2)");
    await until(() => filterCounts().all === 2 && listRows().filter((r) => r.type === "card").length === 2, "account did not scope list and state counts");
    const beforeCounts = JSON.stringify([...pickerCounts().account]);
    for (const query of ["n", "ne", "need", "needle"]) { typeSearch(query); await sleep(30); }
    require(JSON.stringify([...pickerCounts().account]) === beforeCounts, "text search changed picker counts");
    openPicker("accounts"); clickMenu("Personal (1)");
    require(filterCounts().all === 3, "account choices did not combine with OR semantics");
    openPicker("accounts"); clickMenu("Personal (1)");
    await until(() => !contentSearch.busy() && searchItems().some((item) => !!item.found?.hit.archive_id), "content search did not return archive fixtures");
    require(searchItems().every((item) => item.session.accountId === "work"), "account scope lost on metadata/content archive hits");
    require(searchStatus().includes("workstation: contents unavailable"), "an older peer's missing account metadata was reported as complete");
    require(accountFilter().includes(rowBinding(catalog.rows["\nrun:work-a"])), "selection used display account id instead of binding");
    openPicker("machines"); require(buttons(".menu-item").some((b) => b.textContent?.includes("workstation (this machine) (2)")), "machine count ignored account selection");
    clickMenu("All machines");
    openPicker("projects"); require(!!document.querySelector(".menu-label"), "project picker did not render"); clickMenu("All projects");
    typeSearch(""); setSelectedKey("\nrun:work-a"); openInspector("worktrees");
    await until(() => !!document.querySelector(".wt-path"), "Worktrees path row missing");
    document.querySelector<HTMLButtonElement>(".wt-path")!.click();
    require(folderFilter()?.path === "/srv/example/app" && folderFilter()?.checkout === true, "path row did not select exact checkout");
    require(!accountFilter().length, "folder action did not clear account selection as Qt does");
    require(listRows().filter((r) => r.type === "card").length === 2, "exact checkout did not scope list");
    require(selectedKey() === "\nrun:work-a", "filter changed the open conversation");
    openPicker("accounts"); clickMenu("Work (1)"); typeSearch("needle");
    await until(() => !contentSearch.busy() && searchItems().some((item) => !!item.found?.hit.archive_id), "combined content scope missing");
    require(searchItems().every((item) => item.session.accountId === "work" && item.session.gitRoot === "/srv/example/app"), "combined folder/account scope lost on content hits");
    await sleep(200); await log("FOLDER_APPLIED"); await sleep(250);
    buttons(".filter-chips button").find((b) => b.textContent?.startsWith("Folder:"))!.click();
    require(!folderFilter() && accountFilter().length > 0 && filterCounts().all === 2, "folder chip cleared account scope or left the folder active");
    document.querySelector<HTMLButtonElement>(".wt-path")!.click(); openPicker("accounts"); clickMenu("Work (1)");
    setStateFilter("archived");
    require(searchItems().length > 0 && searchItems().every((item) => !!item.session.archiveId && item.session.accountId === "work"), "archive state/account/folder scopes disagreed");
    buttons(".insp-foot button").find((b) => b.textContent === "All sessions")!.click();
    await until(() => !folderFilter() && !accountFilter().length && filterCounts().all === 3, "All sessions did not clear scopes");
    require(document.body.textContent?.includes("The folder filter was cleared"), "mockup clear toast missing");
    setAccountFilter(["unknown-binding"]); openPicker("accounts");
    require(buttons(".menu-item").some((b) => b.textContent?.includes("Unavailable account")), "unknown selected binding was dropped");
    clickMenu("All accounts");
    openPicker("accounts"); await sleep(200);
    await log("PASS (picker interactions, live counts during typing, OR bindings, content/archive scopes, exact checkout, unchanged conversation, clear toast, unavailable account)");
    await sleep(500); await invoke("exit_app", { code: 0 });
  } catch (error) {
    await log(`FAIL ${String(error)} ${JSON.stringify({ accounts: filterAccounts().map((a) => a.label), busy: accountsPage().busy, notices: accountsPage().notices, counts: [...pickerCounts().account], menu: menu()?.items, items: buttons(".menu-item").map((b) => b.textContent) })}`);
    await invoke("exit_app", { code: 1 });
  }
}
