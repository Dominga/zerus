import { describe, expect, it } from "vitest";
import type { ActivityEvent, LocalMessage } from "../../types";
import { buildTranscript } from "./transcript";
import type { ActivityItem } from "./types";

// Synthetic text: the image instruction crosses the old hook-preview boundary.
const path = "/home/example/.local/state/hgs/sessions/attachments/message-ab12/01-image.png";
const echo = `[Image #1] (Inspect the attached image at ${JSON.stringify(path)})`;
const full = `${"Before the image. ".repeat(67)}${echo}\n${"After the image. ".repeat(28)}END OF MESSAGE`;
const clip = (text: string) => Array.from(text.trim()).slice(0, 1200).join("") + "…";
const hook = (detail: string, extra = {}): ActivityEvent => ({ seq: 1, type: "UserPromptSubmit", at: 100, detail, ...extra });
const native = (source: string, detail = full): ActivityEvent => ({ type: "UserMessageBody", source, at: 100.01, detail, message_id: "native-1" });
const items = (events: ActivityEvent[], provider: ActivityEvent[] = [], extra = {}) => buildTranscript(events, { provider_messages: provider, ...extra }) as ActivityItem[];

describe("full own Activity messages", () => {
  it.each(["codex_transcript", "claude_transcript", "kimi_wire"])("restores a clipped hook from %s without adding a row", source => {
    const shown = items([hook(clip(full))], [native(source)]);
    expect(shown).toHaveLength(1);
    expect(shown[0]).toMatchObject({ kind: "user", body: full });
    expect(shown[0].bodyTruncated).not.toBe(true);
  });

  it("uses the retained full hook body while its list preview stays short", () => {
    const event = hook(clip(full), { message_text: full, message_text_truncated: false });
    expect(items([event])[0].body).toBe(full);
    expect(event.detail).toHaveLength(1201);
  });

  it("restores the exact attachment receipt after native hydration", () => {
    const clean = full.replace(echo, "[Image #1]");
    const receipts = [{ source: "hgs_delivery", type: "UserPromptSubmit", message_id: "req-image", submitted_text: full, detail: clean,
      attachments: [{ name: "image.png", mime: "image/png", bytes: 68, reference: "[Image #1]", request_id: "req-image", index: 0 }] }];
    const shown = items([hook(clip(full))], [native("codex_transcript")], { attachment_messages: receipts });
    expect(shown).toHaveLength(1);
    expect(shown[0]).toMatchObject({ body: clean, echoBody: full });
    expect(shown[0].attachments).toHaveLength(1);
  });

  it("recovers a legacy preview from one timed exact receipt even outside the native window", () => {
    const clean = full.replace(echo, "[Image #1]");
    const receipt = { source: "hgs_delivery", type: "UserPromptSubmit", message_id: "req-image", at: 99.5, submitted_text: full, detail: clean,
      attachments: [{ name: "image.png", mime: "image/png", bytes: 68, reference: "[Image #1]", request_id: "req-image", index: 0 }] };
    expect(items([hook(clip(full))], [], { attachment_messages: [receipt] })[0]).toMatchObject({ body: clean, echoBody: full, bodyTruncated: false });
    for (const receipts of [[receipt, { ...receipt, message_id: "other", submitted_text: full + "Other ending" }], [{ ...receipt, at: 80 }], [{ ...receipt, at: undefined }]]) {
      expect(items([hook(clip(full))], [], { attachment_messages: receipts })[0].body).toBe(clip(full));
    }
  });

  it("uses the separate bounded body window, including a Kimi user part after startup context", () => {
    const body = { ...native("kimi_wire"), message_parts: ["Startup hook context", full] };
    const shown = items([hook(clip(full), { type: "TurnStarted", turn_origin: "user" })], [], { user_message_bodies: [body] });
    expect(shown[0].body).toBe(full);
    expect(items([hook(clip(full))], [body], { user_message_bodies: [body] })[0].body).toBe(full);
  });

  it("keeps the local clean message and one row when a full raw hook arrives", () => {
    const clean = full.replace(echo, "[Image #1]");
    const local: LocalMessage = { id: "local-one", text: clean, status: "sent", attachments: [], run_id: "run", conversation_id: "cid",
      submitted_at: 100, error: "", uncertain: false, message_id: "", submitted_text: full, source: null, voice: null };
    const shown = buildTranscript([hook(clip(full), { message_text: full, message_text_truncated: false })], {}, [local]) as ActivityItem[];
    expect(shown).toHaveLength(1);
    expect(shown[0].body).toBe(clean);
    expect(shown[0].bodyTruncated).toBe(false);
  });

  it("keeps ambiguous, unrelated, child and system rows untouched and marks an old preview explicitly", () => {
    const old = hook(clip(full));
    expect(items([old], [native("codex_transcript", full + "OTHER"), native("codex_transcript")])[0].body).toBe(old.detail);
    expect(items([old], [native("codex_transcript", "Different")])[0].bodyTruncated).toBe(true);
    for (const extra of [{ agent_id: "child" }, { origin: "system" }, { inherited_from: "other" }]) {
      expect(items([hook(clip(full), extra)], [native("codex_transcript")])[0].body).not.toBe(full);
    }
    const repeated = items([old, hook(clip(full), { seq: 2, at: 100.5 })], [native("codex_transcript")]);
    expect(repeated.filter(item => item.body === full)).toHaveLength(0);
    expect(repeated).toHaveLength(2);
  });

  it("matches Unicode characters, not UTF-16 units, and does not accept an arbitrary prefix", () => {
    const unicode = "😀".repeat(1210) + "TAIL";
    expect(items([hook(clip(unicode))], [native("claude_transcript", unicode)])[0].body).toBe(unicode);
    expect(items([hook(full.slice(0, 300) + "…")], [native("codex_transcript")])[0].body).not.toBe(full);
  });
});
