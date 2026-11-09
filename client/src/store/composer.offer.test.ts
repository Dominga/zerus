import { describe, expect, it } from "vitest";
import { createRoot } from "solid-js";
import { createComposer, draftKey, type ComposerPort } from "./composer";
import type { ComposerView, SessionRef } from "../types";
import type { Timers } from "./terminal";

// Qt parity (I4): MessageComposer::offerDraft, which Send report uses to put
// the report into the lead's composer. Nothing is sent.

const WORKER: SessionRef = { computer: "studio", key: "run:run-1", name: "claude/orbit/worker", archive_id: null };
const LEAD: SessionRef = { computer: "studio", key: "run:run-2", name: "claude/orbit/lead", archive_id: null };

const view = (session: SessionRef, extra: Partial<ComposerView> = {}): ComposerView => ({
  handle: session === LEAD ? 2 : 1, key: draftKey(session), loaded: true, rev: 0, text_epoch: 1, text: "", position: 0, anchor: 0, question: null, input_visible: true, queue: null,
  attachments: [], removed: null, attaching: false, sending: false, waiting: null, uncertain: false, status: "", status_error: false,
  can_send: true, send_label: { source: "Send" }, available: true, unavailable_reason: "", block_reason: null, attachments_available: true,
  attachments_reason: "", session_status: "", session_status_error: false, session_blocks_input: false, inspect_error: false,
  cache: { visible: false, tone: "quiet", label: "", short_label: "" }, cache_warning: "", cache_details: "",
  context: { visible: false, text: "", tooltip: "", warning: false, tone: "quiet", percent: null, accessible_name: "" }, local: [],
  cancel_send: false, compacting: false,
  clear: { enabled: true, dismiss: false, title: "Clear session?", text: "", confirm: "Clear session", run: "run-1", conversation: "conv-1",
    cache_enabled: true, running: false, reason: "", cache_reason: "" },
  ...extra,
});

const tick = () => new Promise((resolve) => setTimeout(resolve, 0));

function setup(lead: Partial<ComposerView> = {}) {
  const calls: string[] = [];
  const nothing = async () => {};
  const port: ComposerPort = {
    open: async (session) => (session ? view(session, session === LEAD ? lead : {}) : null),
    edit: async (_h, rev, _epoch, text) => { calls.push(`edit ${rev} ${text}`); },
    pick: nothing, attach: nothing, pasteImage: nothing,
    send: async () => { calls.push("send"); return { kind: "sending" }; },
    push: async () => { calls.push("push"); return { kind: "sending" }; },
    sendNow: async () => false, answer: async () => ({ kind: "sending" }),
    remove: nothing, undo: nothing, allowRetry: nothing, cancelSend: nothing, interrupted: nothing, interruptFailed: nothing,
    inboxRemove: async () => ({ outcome: "removed" }),
    clear: async () => ({ kind: "sending" }), refresh: nothing, openAttachment: async () => null,
    localRetry: async () => ({ kind: "sending" }), localDelete: nothing, localInspect: async () => null,
  };
  let clock = 0;
  const jobs: { at: number; fn: () => void }[] = [];
  const timers: Timers = { now: () => clock, later: (fn, ms) => { const job = { at: clock + ms, fn }; jobs.push(job); return () => { const at = jobs.indexOf(job); if (at >= 0) jobs.splice(at, 1); }; } };
  const advance = async (ms: number) => {
    clock += ms;
    for (const job of jobs.filter((x) => x.at <= clock)) { jobs.splice(jobs.indexOf(job), 1); job.fn(); }
    await tick();
  };
  const composer = createRoot(() => createComposer(port, timers, undefined, () => {}));
  return { composer, calls, advance };
}

describe("offering a draft (Send report)", () => {
  it("waits for the lead's composer, then puts the report there with the cursor at its end", async () => {
    const { composer, calls, advance } = setup();
    await composer.open(WORKER);
    const offered = composer.offer(draftKey(LEAD), "Report from Worker:\n\nDone.");
    await advance(50);
    expect(composer.text()).toBe("");
    await composer.open(LEAD);
    await advance(50);
    expect(await offered).toBe(true);
    expect(composer.text()).toBe("Report from Worker:\n\nDone.");
    expect(composer.selection()).toEqual({ start: 26, end: 26 });
    await advance(250);
    expect(calls).toEqual(["edit 1 Report from Worker:\n\nDone."]);
  });

  it("never replaces a draft or a message on its way", async () => {
    for (const lead of [{ text: "My own draft", text_epoch: 2 }, { sending: true }, { recording: true }, { waiting: "Starting the session…" },
      { session_blocks_input: true }, { attachments: [{ name: "a.png", path: "zerus-file:a", mime: "image/png", bytes: 3, thumbnail: false, image_size: null }] as unknown as ComposerView["attachments"] }]) {
      const { composer, calls } = setup(lead);
      await composer.open(LEAD);
      expect(await composer.offer(draftKey(LEAD), "Report"), JSON.stringify(lead)).toBe(false);
      expect(composer.text()).toBe(lead.text ?? "");
      expect(calls).toEqual([]);
    }
  });

  it("gives up when the lead's composer does not open, and refuses an empty report", async () => {
    const { composer, advance } = setup();
    await composer.open(WORKER);
    expect(await composer.offer(draftKey(LEAD), "  \n")).toBe(false);
    expect(await composer.offer("", "Report")).toBe(false);
    let answer: boolean | undefined;
    void composer.offer(draftKey(LEAD), "Report").then((value) => { answer = value; });
    for (let waited = 0; waited < 4000; waited += 50) await advance(50);
    await advance(50);
    expect(answer).toBe(false);
    expect(composer.text()).toBe("");
  });
});
