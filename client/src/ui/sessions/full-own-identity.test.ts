import { expect, it } from "vitest";
import type { ActivityEvent, LocalMessage } from "../../types";
import { restoreOwnMessages } from "./full-own-text";
import { buildTranscript, mergeAttachmentMessages } from "./transcript";
import type { ActivityItem } from "./types";

// Handwritten native/public syntax fixtures; opposite suffixes share a preview.
const prefix = "Common instructions. ".repeat(70).slice(0, 1200);
const a = prefix + " KEEP the file";
const b = prefix + " DELETE the file";
const old: ActivityEvent = { seq: 1, type: "UserPromptSubmit", at: 100, detail: prefix + "…" };
const complete: ActivityEvent = { seq: 2, type: "UserPromptSubmit", at: 105, detail: prefix + "…", message_text: b };
const body: ActivityEvent = { source: "codex_transcript", type: "UserMessageBody", at: 105.01, message_id: "native-b", detail: b };
const file = { name: "image.png", mime: "image/png", bytes: 68, reference: "[Image #1]", request_id: "req-b", index: 0 };
const submitted = b + ' [Image #1] (Inspect the attached image at "/example/attachments/message-ab12/01-image.png")';
const clean = b + " [Image #1]";
const receipt: ActivityEvent = { source: "hgs_delivery", at: 104.9, message_id: "req-b", submitted_text: submitted, detail: clean, attachments: [file] };

it("a complete newer message reserves its own native body", () => {
  const shown = restoreOwnMessages([old, complete], { user_message_bodies: [body] });
  expect(shown[0].message_text).toBeUndefined();
  expect(shown[1].message_text).toBe(b);
});

it("known distinct Kimi turns cannot borrow one another's body", () => {
  const event = { ...old, type: "TurnStarted", turn_origin: "user", kimi_turn_id: 3, kimi_conversation_id: "cid" };
  expect(restoreOwnMessages([event], { user_message_bodies: [{ ...body, source: "kimi_wire", kimi_turn_id: 4, message_parts: [b] }] })[0].message_text).toBeUndefined();
});

it("a locally bound receipt for B cannot also change A through the full projection", () => {
  const local: LocalMessage = { id: "local-b", text: clean, status: "sent", attachments: [file], run_id: "run", conversation_id: "cid",
    submitted_at: 104.9, error: "", uncertain: false, message_id: "req-b", submitted_text: submitted, source: null, voice: null };
  const shown = buildTranscript([old, { ...complete, message_text: submitted }], { attachment_messages: [receipt] }, [local]) as ActivityItem[];
  expect(shown[0].body).toBe(old.detail);
  expect(shown[0].attachments).toBeUndefined();
});

it("an already attached receipt cannot be lent to another row", () => {
  const bound = { ...complete, message_text: clean, attachments: [file], attachment_request_id: "req-b" };
  expect(mergeAttachmentMessages([old, bound], { attachment_messages: [receipt] })[0]).toEqual(old);
});

it("an unbound legacy row cannot claim even the only native body or receipt", () => {
  expect(restoreOwnMessages([old], { user_message_bodies: [body] })[0]).toEqual(old);
  expect(mergeAttachmentMessages([old], { attachment_messages: [receipt] })[0]).toEqual(old);
});

it("a missing B hook leaves A partial and keeps the exact local B as its own row", () => {
  const local: LocalMessage = { id: "local-b", text: clean, status: "sent", attachments: [file], run_id: "run", conversation_id: "cid",
    submitted_at: 104.9, error: "", uncertain: false, message_id: "req-b", submitted_text: submitted, source: null, voice: null };
  const shown = buildTranscript([old], { attachment_messages: [receipt] }, [local]) as ActivityItem[];
  expect(shown).toHaveLength(2);
  expect(shown[0]).toMatchObject({ body: old.detail, bodyTruncated: true, attachments: undefined });
  expect(shown[1].body).toBe(clean);
});

it("a complete conflicting A cannot be overwritten by a nearby local B sharing its prefix", () => {
  const local: LocalMessage = { id: "local-b", text: clean, status: "sent", attachments: [file], run_id: "run", conversation_id: "cid",
    submitted_at: 104.9, error: "", uncertain: false, message_id: "req-b", submitted_text: submitted, source: null, voice: null };
  const shown = buildTranscript([{ ...old, message_text: a }], { attachment_messages: [receipt] }, [local]) as ActivityItem[];
  expect(shown).toHaveLength(2);
  expect(shown[0]).toMatchObject({ body: a, attachments: undefined });
  expect(shown[1].body).toBe(clean);
});

it("native message IDs require the same provider and conversation", () => {
  const event = { ...old, source: "codex_transcript", message_id: "native-a" };
  const exact = { ...body, message_id: "native-a", conversation_id: "cid", detail: a };
  expect(restoreOwnMessages([event], { conversation_id: "cid", user_message_bodies: [exact] })[0].message_text).toBe(a);
  for (const foreign of [{ ...exact, message_id: "native-b" }, { ...exact, conversation_id: "foreign" }, { ...exact, source: "claude_transcript" }]) {
    expect(restoreOwnMessages([event], { conversation_id: "cid", user_message_bodies: [foreign] })[0]).toEqual(event);
  }
  expect(restoreOwnMessages([{ ...event, conversation_id: "foreign" }], { conversation_id: "cid", user_message_bodies: [{ ...exact, conversation_id: "foreign" }] })[0].message_text).toBeUndefined();
});

it("typed Kimi recovery requires exact retained turn and conversation identities", () => {
  const event = { ...old, type: "TurnStarted", turn_origin: "user", kimi_turn_id: 3, kimi_conversation_id: "cid" };
  const exact = { ...body, source: "kimi_wire", kimi_turn_id: 3, kimi_conversation_id: "cid", message_parts: [a] };
  expect(restoreOwnMessages([event], { user_message_bodies: [exact] })[0].message_text).toBe(a);
  for (const changed of [{ ...exact, kimi_turn_id: 4 }, { ...exact, kimi_turn_id: undefined }, { ...exact, kimi_turn_id: "3" }, { ...exact, kimi_conversation_id: "foreign" }, { ...exact, kimi_conversation_id: undefined }]) {
    expect(restoreOwnMessages([event], { user_message_bodies: [changed] })[0]).toEqual(event);
  }
});

it("an exact request binding can recover its own legacy body without timing inference", () => {
  const event = { ...old, attachment_request_id: "req-b", at: 1 };
  expect(mergeAttachmentMessages([event], { attachment_messages: [receipt] })[0]).toMatchObject({ message_text: clean, attachment_request_id: "req-b" });
  const foreign = { ...event, attachment_request_id: "req-other" };
  expect(mergeAttachmentMessages([foreign], { attachment_messages: [receipt] })[0]).toEqual(foreign);
});

it("complete rows participate in one-to-one native identity ownership", () => {
  const shared = { source: "codex_transcript", message_id: "native-b", conversation_id: "cid" };
  const event = { ...old, ...shared };
  const newer = { ...complete, ...shared };
  expect(restoreOwnMessages([event, newer], { user_message_bodies: [{ ...body, ...shared }] })[0]).toEqual(event);
});

it("literal legacy-preview equality cannot establish a receipt binding", () => {
  const echo = '[Image #1] (Inspect the attached image at "/example/attachments/message-ab12/01-image.png")';
  const literal = echo + "x".repeat(1200 - echo.length) + "…";
  const event = { ...old, detail: literal };
  const exact = { ...receipt, submitted_text: literal, detail: "Different complete message" };
  expect(mergeAttachmentMessages([event], { attachment_messages: [exact] })[0]).toEqual(event);
});

it("explicitly partial text cannot establish a receipt binding, but its exact request can", () => {
  const echo = '[Image #1] (Inspect the attached image at "/example/attachments/message-ab12/01-image.png")';
  const literal = echo + "x".repeat(32000 - echo.length) + "…";
  const event = { ...old, message_text: literal, message_text_truncated: true };
  const exact = { ...receipt, submitted_text: literal, detail: "Actual complete text" };
  expect(mergeAttachmentMessages([event], { attachment_messages: [exact] })[0]).toEqual(event);
  const genericPartial = { ...old, detail: literal, truncated: true };
  expect(mergeAttachmentMessages([genericPartial], { attachment_messages: [exact] })[0]).toEqual(genericPartial);
  expect(mergeAttachmentMessages([{ ...event, attachment_request_id: "req-b" }], { attachment_messages: [exact] })[0])
    .toMatchObject({ message_text: "Actual complete text", message_text_truncated: false });
});

it("a copied legacy preview remains distinct from its complete local send", () => {
  const literal = prefix + "…";
  const local: LocalMessage = { id: "local-b", text: literal, status: "sent", attachments: [], run_id: "run", conversation_id: "cid",
    submitted_at: 105, error: "", uncertain: false, message_id: "", submitted_text: literal, source: null, voice: null };
  const shown = buildTranscript([old], {}, [local]) as ActivityItem[];
  expect(shown).toHaveLength(2);
  expect(shown[0]).toMatchObject({ body: old.detail, bodyTruncated: true });
});
