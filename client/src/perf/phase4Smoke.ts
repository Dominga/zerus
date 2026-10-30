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
  disclosure.click();
  await until(atEnd, 3000, "content growth did not stay at the end after send");
  const range = document.createRange();
  range.selectNodeContents(scroller.querySelector(".act-msg")!);
  const selection = document.getSelection()!;
  selection.removeAllRanges();
  selection.addRange(range);
  await sleep(100);
  const selectedTop = scroller.scrollTop;
  disclosure.click();
  await sleep(200);
  if (scroller.scrollTop !== selectedTop) throw new Error("a text selection did not pause Activity follow");
  selection.removeAllRanges();
  await until(atEnd, 3000, "clearing the selection did not resume Activity follow");
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
  const missing = [...document.querySelectorAll<HTMLElement>(".act-msg")].find((row) => row.textContent?.includes("Unreadable image stays raw"));
  if (!missing?.textContent?.includes("Inspect the attached image") || missing.querySelector(".att-card")) {
    throw new Error("an unreadable native image echo did not keep its raw text");
  }
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
  await sleep(3200);
  if (button("Approve once", card).disabled) throw new Error("Approve once stayed disabled after the review");
  button("Approve once", card).click();
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
