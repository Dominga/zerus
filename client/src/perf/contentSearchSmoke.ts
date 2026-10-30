import { invoke } from "@tauri-apps/api/core";
import { activateSearchItem, contentSearch, searchItems, searchStatus, STALE_SEARCH } from "../store/contentSearch";
import { applyBatch, catalog } from "../store/catalog";
import { archive } from "../store/archive";
import { searchPreview } from "../store/searchSelection";
import { selected, setMachineFilter, setPage, setProjectFilter, setSearch, setStateFilter } from "../store/ui";
import { notices } from "../store/notices";
import { emptyBatch } from "../types";

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));
async function until(test: () => boolean, reason: string) {
  const deadline = performance.now() + 15_000;
  while (!test()) { if (performance.now() > deadline) throw Error(reason); await sleep(16); }
}
function require(ok: boolean, reason: string) { if (!ok) throw Error(reason); }

/** Uses only the synthetic replay and explicit fake hgs; never native agents. */
export async function contentSearchSmoke(allowed: boolean) {
  if (!allowed) return;
  const log = (line: string) => invoke("perf_log", { line: `content-search-smoke: ${line}` });
  try {
    await until(() => !!catalog.rows["\nrun:run-review-1"], "replay catalog did not arrive");
    setPage("sessions"); setStateFilter("all"); setSearch("needle");
    await until(() => searchItems().filter((item) => item.found).length === 2 && !contentSearch.busy(), "live and archive contents did not arrive");
    require(new Set(searchItems().map((item) => item.session.key)).size === searchItems().length, "a session appeared more than once");
    require(searchStatus().includes("Limited history searched") && searchStatus().includes(": contents unavailable"), "partial coverage was hidden");
    await until(() => document.querySelectorAll(".content-search-row mark").length >= 2, "excerpts were not highlighted");
    const live = searchItems().find((item) => item.found && !item.session.archiveId)!;
    const archived = searchItems().find((item) => item.found && item.session.archiveId)!;
    require(archived.found!.hit.event.detail === "needle archive exact historical message", "an indexed copy replaced the fresh fallback hit");
    const liveButton = [...document.querySelectorAll<HTMLButtonElement>(".content-search-row")].find((button) => button.textContent?.includes("needle live"));
    liveButton!.click();
    await until(() => searchPreview()?.hit.message_id === "live-match" && !!document.querySelector(".search-preview"), "click did not show the exact live match");
    require(selected()?.runId === "run-review-1", "a click selected another run");
    document.querySelector<HTMLButtonElement>(".search-preview button")!.click();
    require(!searchPreview(), "Return to activity did not clear the preview");
    const list = document.querySelector<HTMLElement>(".content-search-list")!;
    list.focus(); list.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowUp", bubbles: true }));
    require(searchPreview()?.hit.message_id === "live-match", "Arrow navigation did not activate the exact result");
    document.querySelector<HTMLButtonElement>(".search-preview button")!.click();
    list.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    require(searchPreview()?.hit.message_id === "live-match", "Enter did not reactivate the selected result");
    setProjectFilter(["missing-project"]); require(searchItems().length === 0, "project scope was ignored"); setProjectFilter([]);
    setStateFilter("archived"); require(searchItems().every((item) => !!item.session.archiveId), "archive scope included live messages");
    const before = archive.range(); archive.setRange({ period: 4, first: "2000-01-01", last: "2000-01-02" });
    require(searchItems().length === 0, "archive date scope was ignored"); archive.setRange(before);
    activateSearchItem(archived);
    await until(() => selected()?.archiveId === archived.session.archiveId && searchPreview()?.hit.message_id === "archive-match", "archive click lost its exact identity");
    setStateFilter("all");
    // A same-name replacement is not a license to show its latest conversation.
    applyBatch({ ...emptyBatch(), upserts: [{ computer: "", key: "run:run-review-1", row: { name: live.session.name, agent: "claude", run_id: "run-review-1", conversation_id: "replacement", state: "running", status: "ready" } }] });
    activateSearchItem(live);
    require(notices().some((notice) => notice.text === STALE_SEARCH), "stale run/conversation activation was accepted");
    setMachineFilter(["missing-machine"]); require(searchItems().length === 0, "machine scope was ignored"); setMachineFilter([]);
    setSearch(""); require(!searchPreview(), "clearing search retained the excerpt");
    await log("PASS (live/archive excerpts, scopes, date range, click identity, stale refusal, preview return and coverage)");
    await invoke("exit_app", { code: 0 });
  } catch (error) { await log(`FAIL ${String(error)}`); await invoke("exit_app", { code: 1 }); }
}
