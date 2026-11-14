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
    const review = () => Object.values(catalog.rows).find((row) => row.computer === "" && row.name === "claude/sample-project/review");
    await until(() => !!review(), "replay catalog did not arrive");
    setPage("sessions"); setStateFilter("all"); setSearch("needle");
    await until(() => searchItems().filter((item) => item.found).length === 2 && !contentSearch.busy(), "live and archive contents did not arrive");
    // One row per matching message (zerus-5wmh.13.17.1): rows are unique, and a session has at most one metadata row.
    require(new Set(searchItems().map((item) => item.id)).size === searchItems().length, "a row appeared more than once");
    require(searchItems().filter((item) => !item.found).every((item, _, metadata) => metadata.filter((other) => other.session.key === item.session.key).length === 1), "a session had two metadata rows");
    // A partial host names its reason (task 77): no index, an index still building or unreadable sessions.
    require(searchStatus().includes("Limited history searched") && /: (contents unavailable|content index|history unavailable in)/.test(searchStatus()), "partial coverage was hidden");
    await until(() => document.querySelectorAll(".content-search-row mark").length >= 2, "excerpts were not highlighted");
    const metadata = [...document.querySelectorAll(".content-search-meta")].map((node) => node.textContent ?? "");
    require(metadata.some((text) => text.startsWith("Archive / Agent / ")) && metadata.some((text) => text.startsWith("Agent / ")),
      "live/archive result metadata differs from the approved mockup format");
    const cardsAligned = () => {
      const cards = document.querySelectorAll<HTMLElement>(".content-search-row");
      if (cards.length < 2) return false;
      return Math.abs(cards[1].getBoundingClientRect().top - cards[0].getBoundingClientRect().bottom - 2) < 1;
    };
    await until(cardsAligned, "result cards kept estimated heights after messages arrived");
    const text = [".content-search-status", ".content-search-meta", ".content-search-excerpt"].map((selector) => document.querySelector<HTMLElement>(selector)!);
    const fonts = () => text.map((node) => parseFloat(getComputedStyle(node).fontSize));
    const originalFonts = fonts();
    const root = document.documentElement, previousScale = root.style.getPropertyValue("--content-scale");
    root.style.setProperty("--content-scale", "1.5");
    await until(() => fonts().every((font, index) => font > originalFonts[index] * 1.4) && cardsAligned(), "scaled search cards overlap or keep fixed text sizes");
    await log(`CARDS ${JSON.stringify({ metadata, originalFonts, scaledFonts: fonts() })}`);
    await sleep(250); // Allow the private native harness to capture the scaled cards.
    if (previousScale) root.style.setProperty("--content-scale", previousScale); else root.style.removeProperty("--content-scale");
    await until(cardsAligned, "search cards did not remeasure after restoring the scale");
    const live = searchItems().find((item) => item.found && !item.session.archiveId)!;
    const archived = searchItems().find((item) => item.found && item.session.archiveId)!;
    require(archived.found!.hit.event.detail === "needle archive exact historical message", "an indexed copy replaced the fresh fallback hit");
    const liveButton = [...document.querySelectorAll<HTMLButtonElement>(".content-search-row")].find((button) => button.textContent?.includes("needle live"));
    liveButton!.click();
    await until(() => searchPreview()?.hit.message_id === "live-match" && !!document.querySelector(".search-preview"), "click did not show the exact live match");
    require(selected()?.runId === live.run && selected()?.conversationId === live.conversation, "a click selected another run or conversation");
    require(document.querySelector(".search-preview .eyebrow")?.textContent === "SEARCH RESULT — Saved message", "saved-message eyebrow differs from Qt");
    const previewButtons = [...document.querySelectorAll(".search-preview button")];
    require(previewButtons.length === 1 && previewButtons[0].matches('.act-copy[title="Copy as Markdown"]'),
      "the saved message lost Copy as Markdown or gained an invented preview action");
    await until(() => !!document.querySelector('button[aria-label="Back to latest activity"]'), "the existing Latest navigation is unavailable");
    document.querySelector<HTMLButtonElement>('button[aria-label="Back to latest activity"]')!.click();
    require(!searchPreview(), "Latest did not return from the saved message");
    const list = document.querySelector<HTMLElement>(".content-search-list")!;
    list.focus(); list.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowUp", bubbles: true }));
    require(searchPreview()?.hit.message_id === "live-match", "Arrow navigation did not activate the exact result");
    await until(() => !!document.querySelector('button[aria-label="Back to latest activity"]'), "the existing Latest navigation is unavailable");
    document.querySelector<HTMLButtonElement>('button[aria-label="Back to latest activity"]')!.click();
    require(!searchPreview(), "Latest did not return from the saved message");
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
    applyBatch({ ...emptyBatch(), upserts: [{ computer: live.session.computer, key: `run:${live.run}`, row: { name: live.session.name, agent: "claude", run_id: live.run, conversation_id: "replacement", state: "running", status: "ready" } }] });
    activateSearchItem(live);
    require(notices().some((notice) => notice.text === STALE_SEARCH), "stale run/conversation activation was accepted");
    setMachineFilter(["missing-machine"]); require(searchItems().length === 0, "machine scope was ignored"); setMachineFilter([]);
    setSearch(""); require(!searchPreview(), "clearing search retained the excerpt");
    // A real owned slow CLI read must settle when this generation is cancelled.
    const generation = Date.now() + 60_000;
    const pending = invoke("content_search", { computer: "", query: "slow-needle", generation })
      .then(() => false, (error) => String(error) === "superseded");
    await sleep(1000);
    await invoke("cancel_content_search", { through: generation });
    require(await Promise.race([pending, sleep(5000).then(() => false)]), "the cancelled native read did not settle as superseded");
    await log("CANCELLED (owned slow CLI read reaped)");
    await log("PASS (live/archive excerpts, scopes, date range, click identity, stale refusal, scaled cards, saved-message view, Latest navigation and coverage)");
    await invoke("exit_app", { code: 0 });
  } catch (error) { await log(`FAIL ${String(error)}`); await invoke("exit_app", { code: 1 }); }
}
