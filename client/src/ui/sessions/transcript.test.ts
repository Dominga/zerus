import { describe, expect, it } from "vitest";
import { buildTranscript, localFileAvailable, mergeLocal } from "./transcript";
import type { ActivityEvent, LocalMessage } from "../../types";
import type { ActivityItem } from "./types";

const local = (status: "sending" | "error" | "sent", uncertain = false, extra: Partial<LocalMessage> = {}): LocalMessage => ({
  id: `l-${status}-${uncertain}`, text: "hello", status, attachments: [], run_id: "r", conversation_id: "c",
  submitted_at: 1_793_000_000, error: status === "error" ? "hgs: refused" : "", uncertain, message_id: null, submitted_text: null, source: null, voice: null,
  ...extra,
});

describe("local message rows (ActivityView.cpp:665-720, 1053-1068)", () => {
  it("a native queued prompt keeps its queued label before answer labels", () => {
    const events: ActivityEvent[] = [{ seq: 1, type: "UserPromptQueued", at: 10, detail: "also lint",
      question_replies: [{ question: "Which checks?", answer: "Lint" }] }];
    expect(buildTranscript(events)[0]).toMatchObject({ label: "You (queued)", receipt: "Queued at this time" });
    events.push({ seq: 2, type: "UserPromptSubmit", at: 11, detail: "also lint" });
    expect(buildTranscript(events)[0]).toMatchObject({ label: "You (queued)", receipt: "Sent" });
  });

  it("local rows follow Qt's stamps and buttons", () => {
    const items = buildTranscript([], {}, [local("sending"), local("error"), local("error", true), local("sent")]) as ActivityItem[];
    expect(items.map((i) => i.local?.stamp)).toEqual(["Sending…", "Not sent", "Delivery not confirmed", "Submitted to terminal"]);
    expect(items[1].local?.actions).toEqual(["retry", "delete"]);
    expect(items[2].local?.actions).toEqual(["inspect", "delete"]);
    expect(items[2].local?.inspectLabel).toBe("Check Terminal");
    expect(buildTranscript([], { native_ui_available: true }, [local("error", true)])[0]).toMatchObject({ local: { inspectLabel: "Check native UI" } });
  });

  it("only a failed row has buttons and an error line; the stamp stands in for the time", () => {
    const items = buildTranscript([], {}, [local("sending"), local("sent"), local("error")]) as ActivityItem[];
    expect(items.map((i) => i.local?.actions)).toEqual([[], [], ["retry", "delete"]]);
    expect(items.map((i) => i.local?.error)).toEqual(["", "", "hgs: refused"]);
    expect(items.map((i) => [i.id, i.kind, i.at, i.body])).toEqual([
      ["local-l-sending-false", "user", "", "hello"], ["local-l-sent-false", "user", "", "hello"], ["local-l-error-false", "user", "", "hello"],
    ]);
    expect(items.every((i) => i.receipt === undefined)).toBe(true);
  });

  it("a row is placed by its time, after the events of the same time", () => {
    const events: ActivityEvent[] = [
      { seq: 1, type: "Stop", detail: "earlier reply", at: 100 },
      { seq: 2, type: "Stop", detail: "same time", at: 200 },
      { seq: 3, type: "Stop", detail: "later reply", at: 300 },
    ];
    const items = buildTranscript(events, {}, [local("error", false, { submitted_at: 200, text: "mine" })]) as ActivityItem[];
    expect(items.map((i) => i.body)).toEqual(["earlier reply", "same time", "mine", "later reply"]);
    const first = buildTranscript(events, {}, [local("error", false, { submitted_at: 50, text: "first" })]) as ActivityItem[];
    expect(first.map((i) => i.body)).toEqual(["first", "earlier reply", "same time", "later reply"]);
  });

  it("a sent row takes over its own prompt event instead of showing twice", () => {
    const events: ActivityEvent[] = [{ seq: 4, type: "UserPromptSubmit", detail: "hello there", at: 1_793_000_003 }];
    const sent = local("sent", false, { text: "hello there" });
    const items = buildTranscript(events, {}, [sent]) as ActivityItem[];
    expect(items).toHaveLength(1);
    expect(items[0]).toMatchObject({ id: "s4", kind: "user", body: "hello there", receipt: "Sent" });
    // More than 10 s away, or a failed row: never the same message.
    expect(buildTranscript(events, {}, [{ ...sent, submitted_at: 1_793_000_020 }])).toHaveLength(2);
    expect(buildTranscript(events, {}, [local("error", false, { text: "hello there" })])).toHaveLength(2);
    // A subagent's prompt is never this app's.
    expect(buildTranscript([{ ...events[0], agent_id: "a-1" }], {}, [sent]).filter((i) => i.kind === "user")).toHaveLength(1);
  });

  it("a sent row with files binds the one prompt with exactly its text, and its event shows the files", () => {
    const file = { name: "notes.txt", mime: "text/plain", bytes: 2048, reference: "[File #1]", request_id: "req-9", index: 0 };
    const sent = local("sent", false, { text: "see [File #1]", attachments: [file], submitted_text: "see [File #1]" });
    const events: ActivityEvent[] = [{ seq: 7, type: "UserPromptSubmit", detail: "see [File #1]", at: 1_793_000_500 }];
    const merged = mergeLocal(events, [sent]);
    expect(merged).toHaveLength(1);
    expect(merged[0]).toMatchObject({ seq: 7, activity_key: "local-l-sent-false", attachments: [file] });
    // Two such rows compete: neither binds, and both show as rows.
    expect(mergeLocal(events, [sent, { ...sent, id: "other" }])).toHaveLength(3);
    // Two exact events: Qt binds none without its remembered binding.
    expect(mergeLocal([...events, { ...events[0], seq: 8 }], [sent])).toHaveLength(3);
  });

  it("a voice row the journal already holds is left out", () => {
    const voice = local("sent", false, { id: "voice-m-1", source: "hgs_voice", message_id: "m-1", voice: { id: "m-1", state: "submitted", audio: {} } });
    expect(mergeLocal([{ seq: 1, source: "hgs_voice", message_id: "m-1", voice: { id: "m-1" }, at: 1 }], [voice])).toHaveLength(1);
    const items = buildTranscript([], {}, [voice]);
    expect(items).toMatchObject([{ kind: "media", id: "vm-1" }]);
  });

  it("a local row's file is available with its request or its cached copy (Qt's rule)", () => {
    expect(localFileAvailable({ request_id: "req-1" })).toBe(true);
    expect(localFileAvailable({ local_path: "/cache/ab.bin" })).toBe(true);
    expect(localFileAvailable({ request_id: "", local_path: "" })).toBe(false);
    expect(localFileAvailable({})).toBe(false);
    const row = local("error", false, { attachments: [{ name: "a.png", local_path: "/cache/a.bin" }, { name: "b.png" }] });
    expect((buildTranscript([], {}, [row])[0] as ActivityItem).local?.available).toEqual([true, false]);
  });

  it("without events the recorded context comes first, then the local rows", () => {
    const items = buildTranscript([], { prompt: "earlier request" }, [local("error")]) as ActivityItem[];
    expect(items.map((i) => i.kind)).toEqual(["context", "user", "user"]);
    expect(items[2].local?.stamp).toBe("Not sent");
    expect(buildTranscript([], { prompt: "earlier request" })).toHaveLength(2);
  });
});
