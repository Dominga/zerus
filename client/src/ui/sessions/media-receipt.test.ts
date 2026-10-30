import { describe, expect, it } from "vitest";
import { buildTranscript } from "./transcript";
import type { ActivityEvent } from "../../types";
import { cardTarget } from "./attachment-card";

function media(audio: Record<string, unknown>, video?: Record<string, unknown>) {
  return buildTranscript([{ seq: 1, type: "UserPromptSubmit", source: "hgs_voice", message_id: "message", voice: { id: "message", state: "submitted", audio, video } } as ActivityEvent])[0];
}

describe("recording receipt projection", () => {
  it("preserves each exact audio or video attachment identity", () => {
    const audio = { request_id: "audio-request", index: 0 };
    expect(media(audio)).toMatchObject({ kind: "media", media: "voice", file: audio });
    expect(media(audio, { request_id: "video-request", index: 1 })).toMatchObject({ kind: "media", media: "video", file: { request_id: "video-request", index: 1 } });
  });
  it("uses the selected peer and archived conversation, never the client's local host", () => {
    const item = media({ request_id: "audio-request", index: 0 });
    if (item.kind !== "media" || !item.file) throw new Error("missing media receipt");
    expect(cardTarget(item.file, { computer: "studio", name: "codex/example/review", conversation_id: "archived-conversation", archive_id: "archive-1" })).toEqual({
      computer: "studio", ref: { name: "codex/example/review", request_id: "audio-request", index: 0, conversation_id: "archived-conversation", archive_id: "archive-1", agent_id: null },
    });
  });
  it.each([{}, { request_id: "r" }, { request_id: "r", index: null }, { request_id: "r", index: -1 }, { request_id: "r", index: 1.5 }])("never fabricates a missing or malformed receipt: %j", (audio) => {
    expect(media(audio)).not.toHaveProperty("file", expect.any(Object));
  });
  it("keeps a video without its own receipt unavailable, as Qt does", () => {
    expect(media({ request_id: "audio-request", index: 0 }, { duration_ms: 3000 })).not.toHaveProperty("file", expect.any(Object));
  });
});
