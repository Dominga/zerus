import { invoke } from "@tauri-apps/api/core";
import * as bridge from "../bridge";
import { orderedRows } from "../store/catalog";
import { activity } from "../store/session";
import { notices } from "../store/actions";
import { composer } from "../store/composer";
import { terminal } from "../store/terminal";
import { processes } from "../store/processes";
import { questions } from "../store/questions";
import { selectedKey, setDetailTab, setSelectedKey } from "../store/ui";
import { flag, setSetting } from "../store/settings";
import type { Session } from "../ui/sessions/types";
import { activityReadingRows } from "../ui/sessions/activity-anchor";
import { activitySelectionText, activityVirtualOffset } from "../ui/sessions/activity-virtual";


const API = "claude/sample-project/api-refactor";
const PAGINATION = "codex/sample-project/pagination";
const REVIEW = "claude/sample-project/review";
const DOCS = "claude/sample-project/docs-pass";
const FLAKY = "kimi/sample-project/flaky-test";
const RELEASE = "codex/sample-project/release-notes";
const QUESTION_HASH = "0123456789abcdef".repeat(4);
const PNG = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=";


interface SmokeEntry { argv: string[]; stdin: string }
interface AnswerRequest {
  question_id: string;
  expected_question_hash: string;
  expected_run_id: string;
  expected_conversation_id: string;
  answers: { question_id: string; selected_option_ids: string[]; text: string }[];
}
interface QueueRequest { queue_id: string }

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

function until(ready: () => boolean, ms: number, what: string): Promise<void> {
  const started = performance.now();
  return new Promise((resolve, reject) => {
    const check = () => {
      if (ready()) resolve();
      else if (performance.now() - started > ms) reject(new Error(what));
      else setTimeout(check, 16);
    };
    check();
  });
}

async function untilAsync(ready: () => Promise<boolean>, ms: number, what: string): Promise<void> {
  const started = performance.now();
  while (true) {
    if (await ready()) return;
    if (performance.now() - started > ms) throw new Error(what);
    await sleep(40);
  }
}

function log(line: string) {
  void invoke("perf_log", { line }).catch(() => {});
}

async function calls(command: string): Promise<SmokeEntry[]> {
  const entries = await invoke<SmokeEntry[]>("smoke_log");
  return entries.filter((entry) => {
    const args = entry.argv[0]?.startsWith("@") ? entry.argv.slice(1) : entry.argv;
    return args[0] === command;
  });
}

async function waitCall(command: string, count: number, ms = 10_000): Promise<SmokeEntry[]> {
  await untilAsync(async () => (await calls(command)).length >= count, ms, `${command} was not called ${count} times`);
  return calls(command);
}

async function select(name: string): Promise<Session> {
  await until(() => orderedRows().some((row) => row.name === name), 30_000, `${name} did not appear`);
  const row = orderedRows().find((candidate) => candidate.name === name)!;
  setDetailTab("activity");
  setSelectedKey(row.key);
  await until(() => activity.target()?.id === row.key && activity.loaded() && composer.view()?.key === `${row.computer}\n${row.name}` && composer.view()?.loaded === true
    && composer.view()?.unavailable_reason !== "Checking the session…" && composer.view()?.block_reason !== "Checking the session…",
    15_000, `${name} did not open`)
    .catch((error) => {
      const view = composer.view();
      throw new Error(`${error.message}; ${JSON.stringify({ selected: selectedKey(), activityTarget: activity.target()?.id,
        activityLoaded: activity.loaded(), key: view?.key, loaded: view?.loaded, reason: view?.unavailable_reason, block: view?.block_reason })}`);
    });
  return row;
}

function editor(): HTMLTextAreaElement {
  const field = document.querySelector<HTMLTextAreaElement>(".cmp-editor");
  if (!field) throw new Error("the composer editor is missing");
  return field;
}

function type(text: string) {
  const field = editor();
  field.value = text;
  field.dispatchEvent(new InputEvent("input", { bubbles: true }));
}

function button(text: string, root: ParentNode = document): HTMLButtonElement {
  const found = [...root.querySelectorAll<HTMLButtonElement>("button")].find((candidate) => candidate.textContent?.trim() === text);
  if (!found) throw new Error(`button not found: ${text}`);
  return found;
}

function menuAction(text: string): HTMLButtonElement {
  const found = [...document.querySelectorAll<HTMLButtonElement>(".menu button")].find((candidate) => candidate.textContent?.trim().startsWith(text));
  if (!found) throw new Error(`menu action not found: ${text}`);
  return found;
}

async function terminalStep() {
  await select(API);
  setDetailTab("terminal");
  await until(() => terminal.connected(), 15_000, "the Terminal did not connect");
  terminal.input(new TextEncoder().encode("echo smoke\r"));
  await until(() => document.querySelector(".term-host")?.textContent?.includes("echo smoke") === true, 10_000, "the Terminal did not show the echo");
  setDetailTab("activity");
}

async function savedDraftsDialogStep(goToSession = false) {
  const opener = document.querySelector<HTMLButtonElement>('button[aria-label="Saved drafts"]');
  if (!opener) throw new Error("the Saved drafts button is missing");
  opener.click();
  const dialog = () => document.querySelector<HTMLDialogElement>("dialog.saved-drafts");
  await until(() => dialog()?.open === true, 10_000, "Saved drafts did not open");
  if (!goToSession) {
    await until(() => document.activeElement === dialog()?.querySelector("[data-initial]"), 1000, "Saved drafts did not focus Close");
    button("Close", dialog()!).click();
    await until(() => dialog()?.open === false && document.activeElement === opener, 1000, "Saved drafts did not return focus");
    return;
  }
  await until(() => button("Go to session", dialog()!).getAttribute("aria-disabled") !== "true", 5000, "Go to session did not become available");
  button("Go to session", dialog()!).click();
  await until(() => dialog()?.open === false && document.activeElement === document.querySelector(".cmp-editor"), 5000, "Go to session did not focus the editor");
  const reopened = document.querySelector<HTMLButtonElement>('button[aria-label="Saved drafts"]');
  if (!reopened) throw new Error("the Saved drafts button is missing after Go to session");
  reopened.click();
  await until(() => dialog()?.open === true, 10_000, "Saved drafts did not reopen");
  button("Close", dialog()!).click();
  await until(() => dialog()?.open === false && document.activeElement === reopened, 1000, "Saved drafts did not return focus")
    .catch((error) => {
      const active = document.activeElement;
      throw new Error(`${error.message}; ${JSON.stringify({ active: active?.outerHTML.slice(0, 500), openerConnected: reopened.isConnected, dialogOpen: dialog()?.open, composerKey: composer.view()?.key, composerLoaded: composer.view()?.loaded })}`);
    });
}

async function draftsStep() {
  await select(API);
  type("phase 4 smoke");
  await until(() => composer.text() === "phase 4 smoke", 1000, "the editor did not take the text");
  await sleep(350);
  await select(PAGINATION);
  await select(API);
  if (composer.text() !== "phase 4 smoke") throw new Error("the draft did not survive navigation");
}

async function attachStep(path: string) {
  await select(API);
  const view = composer.view()!;
  const at = composer.selection().end;
  await bridge.composerAttach(view.handle, [path], at, at);
  await until(() => composer.view()?.attachments.some((file) => file.reference === "[File #1]") === true, 10_000, "the file did not attach");
}

async function sendStep(path: string) {
  await select(API);
  if (!composer.sendEnabled()) {
    const view = composer.view();
    throw new Error(`Send is disabled: ${JSON.stringify({ status: composer.status(), locked: composer.locked(), operationLocked: composer.operationLocked(),
      available: view?.available, reason: view?.unavailable_reason, block: view?.block_reason, attachments: view?.attachments.length,
      attachmentsAvailable: view?.attachments_available, attachmentReason: view?.attachments_reason, text: composer.text(),
      catalogTracked: orderedRows().find((row) => row.name === API)?.tracked })}`);
  }
  await composer.send();
  await until(() => composer.view()?.local.some((row) => row.status === "sent") === true, 10_000, "the local submitted row did not appear")
    .catch((error) => {
      const view = composer.view();
      throw new Error(`${error.message}; ${JSON.stringify({ status: view?.status, canSend: view?.can_send, sending: view?.sending,
        waiting: view?.waiting, uncertain: view?.uncertain, local: view?.local.map((row) => row.status), notices: notices().map((notice) => notice.text) })}`);
    });
  const submitted = composer.view()?.local.find((row) => row.status === "sent");
  if (!submitted || submitted.uncertain || submitted.error) throw new Error("the submitted row is not settled");
  const sent = await waitCall("send", 1);
  const name = path.split("/").pop()!;
  if (!sent[0].stdin.includes(name)) throw new Error("the attachment name is not in the send payload");
  if (sent[0].argv.some((word) => word.includes("phase 4 smoke"))) throw new Error("message text appeared in argv");
}

async function sendFollowStep() {
  await select(API);
  const scroller = document.querySelector<HTMLDivElement>(".act-scroll")!;
  await until(() => scroller.scrollHeight > scroller.clientHeight + 200, 10_000, "Activity is too short for the follow check");
  scroller.dispatchEvent(new WheelEvent("wheel", { deltaY: -300, bubbles: true, cancelable: true }));
  scroller.scrollTo({ top: 0, behavior: "instant" });
  await sleep(100);
  type("follow smoke\nsecond line\nthird line");
  await composer.send();
  await until(() => composer.view()?.local.some((row) => row.status === "sent" && row.text === "follow smoke\nsecond line\nthird line") === true,
    10_000, "the follow-check send did not settle");
  const atEnd = () => Math.abs(scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight) < 2;
  await until(atEnd, 3000, "the sent row did not return to the end");
  const disclosure = scroller.querySelector<HTMLButtonElement>(".act-tools-head");
  if (!disclosure) throw new Error("the follow check needs a tool disclosure");
  const disclosureId = disclosure.closest<HTMLElement>(".act-tools")!.id;
  const disclosureStart = parseFloat(disclosure.closest<HTMLElement>(".act-virtual-row")!.style.transform.replace("translateY(", ""));
  disclosure.click();
  await until(atEnd, 3000, "content growth did not stay at the end after send");
  const range = document.createRange();
  range.selectNodeContents(scroller.querySelector(".act-msg")!);
  const selection = document.getSelection()!;
  selection.removeAllRanges();
  selection.addRange(range);
  await sleep(100);
  const selectedTop = scroller.scrollTop, selectedHeight = scroller.scrollHeight;
  const selectedText = selection.toString();
  const originalScrollTo = scroller.scrollTo, scrollTo = originalScrollTo.bind(scroller);
  const writes: unknown[] = [];
  const frames: { top: number; height: number }[] = [];
  scroller.scrollTo = (options?: ScrollToOptions | number, y?: number) => {
    writes.push({ options, y });
    if (typeof options === "number") scrollTo(options, y ?? 0);
    else scrollTo(options);
  };
  try {
    disclosure.click();
    // Hold longer than the gesture idle timeout. New journal events must not
    // alter layout or invalidate the selected range until it is cleared.
    const target = activity.target()!;
    const seq = Math.max(0, ...activity.events().map((event) => event.seq ?? 0)) + 1;
    for (let n = 0; n < 30; n++) {
      if (n % 5 === 0) activity.applyActivity([{ computer: target.computer, key: target.key, kind: "events", payload: {
        events: [{ seq: seq + n, type: "Stop", source: "claude_hooks", detail: `Selection hold smoke ${n}`, at: 1_793_000_000 + seq + n }],
      } }]);
      await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
      frames.push({ top: scroller.scrollTop, height: scroller.scrollHeight });
      log(`phase4-selection-frame ${JSON.stringify({ n, top: scroller.scrollTop, height: scroller.scrollHeight })}`);
    }
    await sleep(300);
    log(`phase4-selection-hold ${JSON.stringify({ selectedTop, selectedHeight, writes, frames: frames.length })}`);
    if (writes.length || frames.some((frame) => frame.top !== selectedTop || frame.height !== selectedHeight)
      || scroller.scrollTop !== selectedTop) throw new Error("a text selection did not pause Activity follow");
    if (selection.toString() !== selectedText) throw new Error("streaming invalidated the Activity selection");
  } finally {
    scroller.scrollTo = originalScrollTo;
    selection.removeAllRanges();
  }
  await until(() => atEnd() && (!disclosure.isConnected || disclosure.getAttribute("aria-expanded") === "false")
    && scroller.textContent?.includes("Selection hold smoke 25") === true, 3000, "clearing the selection did not resume Activity follow")
    .catch((error) => {
      throw new Error(`${error.message}: ${JSON.stringify({ atEnd: atEnd(), expanded: disclosure.getAttribute("aria-expanded"), disclosureConnected: disclosure.isConnected,
        appended: scroller.textContent?.includes("Selection hold smoke 25"), top: scroller.scrollTop, height: scroller.scrollHeight, viewport: scroller.clientHeight,
        placed: scroller.querySelector<HTMLDivElement>(".act-col")?.style.visibility, rows: scroller.querySelectorAll(".act-virtual-row").length })}`);
    });
  if (!disclosure.isConnected) {
    scroller.dispatchEvent(new WheelEvent("wheel", { deltaY: -300, bubbles: true, cancelable: true }));
    scroller.scrollTo({ top: disclosureStart, behavior: "instant" });
    await until(() => document.getElementById(disclosureId)?.querySelector(".act-tools-head")?.getAttribute("aria-expanded") === "false",
      3000, "the collapsed disclosure did not retain its state after virtual remounting");
  }
  const again = document.createRange();
  again.selectNodeContents(scroller.querySelector(".act-msg")!);
  selection.addRange(again);
  await sleep(100);
  await select(PAGINATION);
  await until(() => {
    const next = document.querySelector<HTMLDivElement>(".act-scroll");
    return next?.querySelector<HTMLDivElement>(".act-col")?.style.visibility === "visible" && selection.isCollapsed
      && Math.abs(next.scrollHeight - next.scrollTop - next.clientHeight) < 2;
  }, 3000, "the old selection blocked the next session's first placement");
}

async function activityAnchorStep() {
  await setSetting("workspace/reduceMotion", true, { persist: false });
  await select(DOCS);
  const scroller = document.querySelector<HTMLDivElement>(".act-scroll")!;
  await until(() => scroller.querySelectorAll(".act-virtual-row").length > 0, 10_000, "the virtual Activity did not mount");
  const before = activity.events()[0]?.seq ?? 0;
  await until(() => scroller.scrollTop > 0, 5000, "Activity did not reach its initial end position");
  const animated = scroller.querySelector<HTMLElement>(".act-virtual-row")!;
  await until(() => animated.getAnimations({ subtree: true }).length === 0, 3000, "the virtual row animation did not settle");
  const row = scroller.querySelector<HTMLElement>(".act-virtual-row")!;
  const start = parseFloat(row.style.transform.replace("translateY(", ""));
  const key = row.querySelector<HTMLElement>("[id^=item-]")!.id;
  const formula = activityVirtualOffset(scroller, row.parentElement!, start), dom = activityReadingRows(scroller).offset(key);
  if (scroller.scrollTop < 1000 || dom === undefined || Math.abs(formula - dom) >= 1) {
    throw new Error(`the virtual measured formula disagrees with the DOM: ${JSON.stringify({ formula, dom, scrollTop: scroller.scrollTop })}`);
  }
  scroller.dispatchEvent(new WheelEvent("wheel", { deltaY: -300, bubbles: true, cancelable: true }));
  scroller.scrollTo({ top: 0, behavior: "instant" });
  await new Promise(requestAnimationFrame);
  const marker = activityReadingRows(scroller).visible()[0];
  if (!marker) throw new Error("the anchor check has no visible item");
  const id = marker.key, top = marker.offset;
  await until(() => (activity.events()[0]?.seq ?? before) < before, 10_000, "the older Activity page did not arrive");
  await until(() => {
    const offset = activityReadingRows(scroller).offset(id);
    return offset !== undefined && Math.abs(offset - top) < 3;
  }, 3000, "the older-page anchor did not settle")
    .catch((error) => {
      throw new Error(`${error.message}: ${JSON.stringify({ id, top, captured: marker.offset, currentTop: activityReadingRows(scroller).offset(id),
        scrollTop: scroller.scrollTop, height: scroller.scrollHeight, rows: scroller.querySelectorAll(".act-virtual-row").length })}`);
    });
}

async function readingAppendStep() {
  const scroller = document.querySelector<HTMLDivElement>(".act-scroll")!;
  document.querySelector<HTMLButtonElement>('button[aria-label^="Jump to latest activity"]')?.click();
  await until(() => scroller.scrollTop > 1000, 5000, "Activity did not return to a deep reading position");
  const before = scroller.scrollTop;
  scroller.focus();
  scroller.dispatchEvent(new KeyboardEvent("keydown", { key: "PageUp", bubbles: true, cancelable: true }));
  // Untrusted keys do not scroll WebKit, so apply the Page Up viewport movement explicitly.
  scroller.scrollBy({ top: -scroller.clientHeight, behavior: "instant" });
  await until(() => scroller.scrollTop < before, 3000, "Page Up did not move Activity")
    .catch((error) => {
      throw new Error(`${error.message}: ${JSON.stringify({ before, after: scroller.scrollTop, active: document.activeElement?.className })}`);
    });
  const edge = () => scroller.getBoundingClientRect().top + scroller.clientTop;
  await until(() => [...scroller.querySelectorAll<HTMLElement>(".act-virtual-row [id^=item-]")].some((item) => {
    const rect = item.getBoundingClientRect();
    return rect.bottom > edge() && rect.top < edge() + scroller.clientHeight;
  }), 3000, "Page Up did not render a reading row");
  const marker = [...scroller.querySelectorAll<HTMLElement>(".act-virtual-row [id^=item-]")].find((item) => {
    const rect = item.getBoundingClientRect();
    return rect.bottom > edge() && rect.top < edge() + scroller.clientHeight;
  });
  if (!marker) throw new Error("the append check has no visible reading row");
  const id = marker.id, top = marker.getBoundingClientRect().top - edge();
  const seq = Math.max(...activity.events().map((event) => event.seq ?? 0)) + 1;
  const at = Math.max(...activity.events().map((event) => event.at ?? 0)) + 1;
  activity.applyActivity([{ computer: "", key: "run:000003-run", kind: "events", payload: {
    events: [{ seq, type: "Stop", source: "claude_transcript", detail: "append while reading smoke", at }], cursor: seq,
  } }]);
  await until(() => activity.events().some((event) => event.seq === seq), 1000, "the appended event did not arrive");
  await until(() => {
    const current = document.getElementById(id);
    return !!current && Math.abs(current.getBoundingClientRect().top - edge() - top) < 1;
  }, 3000, "an append moved the released reading anchor")
    .catch((error) => {
      const current = document.getElementById(id);
      throw new Error(`${error.message}: ${JSON.stringify({ id, top, currentTop: current ? current.getBoundingClientRect().top - edge() : undefined,
        scrollTop: scroller.scrollTop, height: scroller.scrollHeight, rows: scroller.querySelectorAll(".act-virtual-row").length })}`);
    });
}

async function selectAllCopyStep() {
  const scroller = document.querySelector<HTMLDivElement>(".act-scroll")!;
  document.querySelector<HTMLButtonElement>('button[aria-label^="Jump to latest activity"]')?.click();
  const users = activity.events().filter((event) => event.type === "UserPromptSubmit" && event.detail);
  const first = users[0]?.detail, last = users.at(-1)?.detail;
  if (!first || !last) throw new Error("the copy check needs loaded user messages at both ends");
  // Repeated passing notices coalesce on the bus, so count copies by their notice ids.
  const copyIds = new Set<number>();
  const copyNotices = () => {
    for (const notice of notices()) if (notice.text === "The selected text is on the clipboard") copyIds.add(notice.id);
    return copyIds.size;
  };
  const writes: string[] = [];
  scroller.focus();
  scroller.dispatchEvent(new KeyboardEvent("keydown", { key: "a", ctrlKey: true, bubbles: true, cancelable: true }));
  const mounted = document.getSelection()?.toString() ?? "";
  scroller.dispatchEvent(new KeyboardEvent("keydown", { key: "c", ctrlKey: true, bubbles: true, cancelable: true }));
  await until(() => copyNotices() === 1, 3000, "Ctrl+C did not copy the transcript");
  writes.push(activitySelectionText(scroller));
  if (!writes[0].includes(first) || !writes[0].includes(last) || writes[0].length <= mounted.length) {
    throw new Error(`Select All copied only the mounted window: ${JSON.stringify({ copied: writes[0].length, mounted: mounted.length })}`);
  }
  scroller.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: 400, clientY: 300 }));
  await until(() => !!document.querySelector(".menu"), 1000, "the Activity menu did not open");
  menuAction("Select All").click();
  const menuText = activitySelectionText(scroller);
  scroller.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: 400, clientY: 300 }));
  await until(() => !!document.querySelector(".menu"), 1000, "the Activity menu did not reopen");
  menuAction("Copy").click();
  await until(() => copyNotices() === 2, 3000, "the Activity menu did not copy the transcript");
  writes.push(menuText);
  if (writes[1] !== writes[0]) throw new Error(`menu Copy and Ctrl+C returned different full transcripts: ${JSON.stringify({ keyboard: writes[0].length, menu: writes[1].length,
    keyboardFirst: writes[0].includes(first), keyboardLast: writes[0].includes(last), menuFirst: writes[1].includes(first), menuLast: writes[1].includes(last) })}`);

  const range = document.createRange();
  range.selectNodeContents(scroller.querySelector(".act-msg")!);
  const selection = document.getSelection()!;
  selection.removeAllRanges();
  selection.addRange(range);
  const partial = selection.toString();
  const nativeCopy = scroller.dispatchEvent(new KeyboardEvent("keydown", { key: "c", ctrlKey: true, bubbles: true, cancelable: true }));
  if (!nativeCopy) throw new Error("a partial selection was intercepted instead of using native copy");
  await sleep(50);
  if (copyNotices() !== 2) throw new Error("a partial selection used the custom clipboard path");
  if (!partial || partial === writes[0]) throw new Error("the partial selection did not replace Select All");
  selection.removeAllRanges();
}

async function mediaPlaybackStep() {
  await select(API);
  const scroller = document.querySelector<HTMLDivElement>(".act-scroll")!;
  await until(() => !!document.querySelector(".act-msg.media"), 5000, "the playback media row did not appear");
  const card = document.querySelector<HTMLElement>(".act-msg.media")!;
  card.querySelector<HTMLButtonElement>('button[aria-label="Show transcript"]')?.click();
  const play = card.querySelector<HTMLButtonElement>(".vp-btn")!;
  const position = () => Number(card.querySelector(".vp-wave")!.getAttribute("aria-valuenow"));
  play.click();
  await until(() => position() > 0 && play.getAttribute("aria-pressed") === "true", 2000, "voice playback did not start");
  const before = position();
  scroller.dispatchEvent(new WheelEvent("wheel", { deltaY: -300, bubbles: true, cancelable: true }));
  scroller.scrollTo({ top: 0, behavior: "instant" });
  await sleep(350);
  if (!card.isConnected || document.getElementById(card.id) !== card || play.getAttribute("aria-pressed") !== "true" || position() <= before) {
    throw new Error(`scrolling stopped or reset the playing row: ${JSON.stringify({ before, after: position(), connected: card.isConnected, pressed: play.getAttribute("aria-pressed") })}`);
  }
  if (card.querySelector(".vm-text")?.textContent !== "media smoke transcript") throw new Error("scrolling closed the media transcript");
  scroller.scrollTo({ top: scroller.scrollHeight, behavior: "instant" });
  await until(() => {
    const rect = card.getBoundingClientRect();
    return card.isConnected && document.getElementById(card.id) === card && rect.top >= 0 && rect.bottom <= innerHeight;
  }, 3000, "the playing row did not return to the viewport");
  const stop = card.querySelector<HTMLButtonElement>(".vp-btn")!;
  stop.click();
  await until(() => stop.getAttribute("aria-pressed") === "false", 1000, "playback did not stop after the check")
    .catch((error) => {
      throw new Error(`${error.message}: ${JSON.stringify({ pressed: stop.getAttribute("aria-pressed"), position: position(), connected: card.isConnected })}`);
    });
}

async function coldStep() {
  await select(PAGINATION);
  type("cold cache smoke");
  await composer.send();
  await until(() => document.querySelector("#composer-dlg-title")?.textContent === "Continue with a cold cache?", 5000, "the cold-cache question did not open");
  button("Send with full context", document.querySelector("dialog[open]")!).click();
  const sent = await waitCall("send", 2, 15_000);
  if (!sent[1].stdin.includes("cold cache smoke")) throw new Error("the full-context send has the wrong payload");
}

async function startStep() {
  const row = await select(REVIEW);
  type("resume smoke");
  await composer.send();
  await until(() => document.querySelector("#composer-dlg-title")?.textContent === "Start the session to send?", 5000, "the start question did not open");
  button("Resume and send", document.querySelector("dialog[open]")!).click();
  await waitCall("resume", 1);
  await invoke("smoke_resume", { key: row.catalogKey });
  await until(() => !!composer.view()?.waiting || composer.view()?.local.some((row) => row.status === "sent") === true, 3000, "the waiting state did not appear");
  const sent = await waitCall("send", 3, 20_000)
    .catch((error) => {
      const view = composer.view();
      throw new Error(`${error.message}; ${JSON.stringify({ status: view?.status, waiting: view?.waiting, block: view?.block_reason,
        local: view?.local.map((row) => row.status), dialog: !!document.querySelector("dialog.dlg[open]"), notices: notices().map((notice) => notice.text) })}`);
    });
  if (!sent[2].stdin.includes("resume smoke")) throw new Error("the resumed send has the wrong payload");
}

async function processesStep() {
  await select(API);
  await setSetting("processes/enabled", true);
  document.querySelector<HTMLElement>(".scard.selected")!.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: 400, clientY: 300 }));
  await until(() => [...document.querySelectorAll<HTMLButtonElement>("button")].some((candidate) => candidate.textContent?.trim() === "Show running commands"), 3000, "the background-job menu entry did not appear");
  const commands = button("Show running commands");
  if (commands.disabled) throw new Error("the background-job menu entry stayed disabled with live jobs and Processes on");
  commands.click();
  await until(() => processes.view()?.groups.length === 3, 15_000, "the menu did not open the process groups");
  await until(() => flag("processes/enabled") && processes.view()?.groups.length === 3, 15_000, "the three process groups did not appear")
    .catch((error) => {
      const view = processes.view();
      throw new Error(`${error.message}; ${JSON.stringify({ enabled: flag("processes/enabled"), selected: selectedKey(), summary: view?.summary,
        failed: view?.failed, groups: view?.groups.map((group) => [group.group, group.rows.length]) })}`);
    });
  processes.click("call-git-fetch", { toggle: false, range: false });
  await until(() => processes.view()?.detail?.output?.includes("running 42 tests") === true, 10_000, "the process output did not arrive");
  await setSetting("processes/enabled", false);
  setDetailTab("activity");
}

async function previewStep() {
  const row = await select(API);
  await until(() => [...document.querySelectorAll(".att-card")].some((card) => card.textContent?.includes("orders.json")), 10_000, "the JSON attachment card is missing");
  const fetched = await bridge.attachmentFetch(row.computer, { name: row.name, request_id: "req-fixture-1", index: 0,
    conversation_id: row.conversationId ?? "", archive_id: null, agent_id: null });
  if (fetched.mime !== "application/json" || fetched.name !== "orders.json") throw new Error("the JSON attachment read is wrong");
  await until(() => {
    const image = document.querySelector<HTMLImageElement>(".att-card .att-thumb img");
    return !!image && image.src.startsWith("zerus-file:") && image.complete && image.naturalWidth === 1;
  }, 10_000, "the convertFileSrc thumbnail did not decode");
  await until(() => {
    const message = [...document.querySelectorAll<HTMLElement>(".act-msg")].find((row) => row.textContent?.includes("Reconciled image message"));
    const image = message?.querySelector<HTMLImageElement>(".att-card .att-thumb img");
    return !!message && !message.textContent?.includes("Inspect the attached image")
      && !!image && image.src.startsWith("zerus-file:") && image.complete && image.naturalWidth === 1;
  }, 10_000, "the native image echo did not become a decoded attachment card");
  await until(() => {
    const missing = [...document.querySelectorAll<HTMLElement>(".act-msg")].find((row) => row.textContent?.includes("Unreadable image stays raw"));
    return !!missing?.textContent?.includes("Inspect the attached image") && !missing.querySelector(".att-card");
  }, 10_000, "an unreadable native image echo did not keep its raw text");
  const reads = await calls("attachment");
  for (const request of ["req-fixture-echo-image", "req-fixture-missing-image"]) {
    if (!reads.some((entry) => entry.argv.includes(request))) throw new Error(`the echo did not use its guarded read: ${request}`);
  }
}

async function questionStep() {
  const row = await select(DOCS);
  await until(() => document.querySelector(".qcard-title")?.textContent === "Agent needs your answer" && composer.view()?.input_visible === false,
    10_000, "the required question card did not hide the editor");
  const card = document.querySelector(".qcard")!;
  const unit = [...card.querySelectorAll<HTMLElement>("label.qchoice")].find((choice) => choice.textContent?.trim() === "Unit");
  if (!unit) throw new Error("the Unit option is missing");
  const input = unit.querySelector("input")!;
  await until(() => !input.disabled, 10_000, "the Unit option stayed disabled");
  input.click();
  await until(() => input.checked, 3000, "the Unit option was not selected");
  await until(() => !button("Submit answer", card).disabled, 5000, "Submit stayed disabled");
  button("Submit answer", card).click();
  const sent = await waitCall("answer", 1);
  const request = JSON.parse(sent[0].stdin) as AnswerRequest;
  if (request.question_id !== "tool:fixture-1" || request.expected_question_hash !== QUESTION_HASH
    || request.expected_run_id !== row.runId || request.expected_conversation_id !== row.conversationId) {
    throw new Error("the answer has the wrong question identity");
  }
  if (request.answers.length !== 1 || request.answers[0].question_id !== "q_0"
    || request.answers[0].selected_option_ids.join(",") !== "opt_0_0" || request.answers[0].text !== "") {
    throw new Error("the answer has the wrong selected option");
  }
  await until(() => document.querySelector(".qcard-status")?.textContent === "Answer sent. Waiting for the agent…", 5000, "the answer status did not appear");
}

async function approvalStep() {
  await select(FLAKY);
  await until(() => !!document.querySelector(".qcard"), 10_000, "the approval card did not appear");
  questions.setActive(true);
  const card = document.querySelector(".qcard")!;
  await until(() => [...card.querySelectorAll("button")].some((candidate) => candidate.textContent?.trim() === "Approve once (3)"), 3000, "the review countdown did not start");
  if (!button("Approve once (3)", card).disabled) throw new Error("Approve once was enabled during the review");
  const approve = () => [...card.querySelectorAll<HTMLButtonElement>("button")].find((candidate) => candidate.textContent?.trim() === "Approve once");
  await until(() => !!approve() && !approve()!.disabled, 5000, "Approve once stayed disabled after the review");
  approve()!.click();
  const sent = await waitCall("answer", 2);
  const request = JSON.parse(sent[1].stdin) as AnswerRequest;
  if (request.answers.length !== 1 || request.answers[0].selected_option_ids.join(",") !== "allow") {
    throw new Error("the approval has the wrong decision");
  }
}

async function queueStep() {
  await select(RELEASE);
  await until(() => document.querySelector(".act-queue")?.textContent?.includes("↳ also update the changelog") === true,
    10_000, "the native queue did not appear");
  const panel = document.querySelector(".act-queue")!;
  if (button("Send now", panel).disabled) throw new Error("Send now is disabled");
  button("Send now", panel).click();
  const sent = await waitCall("send-now", 1);
  const request = JSON.parse(sent[0].stdin) as QueueRequest;
  if (request.queue_id !== "queue-fixture") throw new Error("Send now names the wrong queue");
  await until(() => notices().some((notice) => notice.text === "Send now requested. Waiting for the agent to accept the queue."),
    5000, "the Send now notice did not appear");
}

async function imagePasteStep() {
  await select(API);
  const bytes = Uint8Array.from(atob(PNG), (char) => char.charCodeAt(0));
  await composer.pasteImage(new Blob([bytes], { type: "image/png" }));
  await until(() => composer.view()?.attachments.some((file) => file.reference === "[Image #1]") === true, 10_000, "the pasted image did not attach");
  button("1 attached").click();
  await until(() => {
    const image = document.querySelector<HTMLImageElement>(".att-row .att-thumb img");
    return !!image && image.src.startsWith("zerus-file:") && image.complete && image.naturalWidth === 1;
  }, 10_000, "the draft thumbnail did not decode");
  document.querySelector(".att-pop")?.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
}

async function clearCloseOrderStep() {
  await select(API);
  composer.openClear();
  await until(() => document.querySelector("#composer-dlg-title")?.textContent === "Clear session?", 3000, "the Clear dialog did not open");
  button("Clear session", document.querySelector("dialog[open]")!).click();
  await until(() => !document.querySelector("dialog.dlg[open]"), 1000, "the Clear dialog stayed open");
  await until(() => composer.view()?.clear.running === true, 1000, "the clear did not start");
  if (document.querySelector("dialog.dlg[open]")) throw new Error("the Clear dialog reopened while the command ran");
  await waitCall("clear-context", 1);
  await until(() => composer.view()?.clear.running === false, 5000, "the clear did not finish");
}

export async function phase4Smoke(allowed: boolean, smokeFile?: string) {
  const exit = (code: number) => invoke("exit_app", { code }).catch(() => {});
  if (!allowed) {
    log("phase4-smoke not started: it needs ZERUS_CLIENT_REPLAY, ZERUS_CLIENT_HGS and ZERUS_CLIENT_PERF");
    return;
  }
  const steps: [string, () => Promise<void>][] = [
    ["terminal", terminalStep],
    ["saved-drafts-dialog", () => savedDraftsDialogStep()],
    ["drafts", draftsStep],
    ["saved-drafts-go-to", () => savedDraftsDialogStep(true)],
    ["attach", () => attachStep(smokeFile!)],
    ["send", () => sendStep(smokeFile!)],
    ["cold", coldStep],
    ["start", startStep],
    ["processes", processesStep],
    ["preview", previewStep],
    ["question", questionStep],
    ["approval", approvalStep],
    ["queue", queueStep],
    ["image-paste", imagePasteStep],
    ["clear-close-order", clearCloseOrderStep],
    ["send-follow", sendFollowStep],
    ["activity-anchor", activityAnchorStep],
    ["reading-append", readingAppendStep],
    ["select-all-copy", selectAllCopyStep],
    ["media-playback", mediaPlaybackStep],
  ];
  let current = "start";
  try {
    if (!smokeFile) throw new Error("ZERUS_CLIENT_SMOKE_FILE is not set");
    for (const [name, run] of steps) {
      current = name;
      await run();
      log(`phase4-smoke ${name}: done`);
    }
    await exit(0);
  } catch (error) {
    log(`phase4-smoke ${current}: failed (${String(error)})`);
    await exit(1);
  }
}
