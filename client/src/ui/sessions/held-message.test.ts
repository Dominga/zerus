import { describe, expect, it } from "vitest";
import { heldMessage, withoutHeld } from "./held-message";
import { buildTranscript } from "./transcript";
import { draftKey } from "../../store/draftKeys";
import type { LocalMessage } from "../../types";
import type { Session } from "./types";

const session = { key: "\nrun:run", catalogKey: "run:run", computer: "", name: "claude/example/review", runId: "run", conversationId: "conversation",
  sessionId: "11111111-1111-4111-8111-111111111111" } as Session;
const key = draftKey({ computer: "", key: session.catalogKey, name: session.name, archive_id: null, session_id: session.sessionId ?? null });
const held = { request_id: "send-1", text: "Please continue with the migration", attachments: ["plan.md"] };
const row = (id: string, text: string): LocalMessage => ({ id, text, status: "sending", attachments: [], run_id: "run", conversation_id: "conversation",
  submitted_at: 1_793_000_000, error: "", uncertain: false, message_id: null, submitted_text: null, source: null, voice: null });

describe("a Compact and continue message held while the agent compacts (zerus-pabah)", () => {
  it("is shown for exactly the selected session's main composer", () => {
    expect(heldMessage({ key, subagent: null, held }, session, false)).toEqual(held);
    expect(heldMessage({ key, subagent: null, held: null }, session, false)).toBeUndefined();
    expect(heldMessage({ key, subagent: null, held }, undefined, false)).toBeUndefined();
    // Never another session's, a child's, or an archived session's.
    expect(heldMessage({ key: `${key}-other`, subagent: null, held }, session, false)).toBeUndefined();
    expect(heldMessage({ key, subagent: { agent_id: "child", send_supported: true, hint: "" }, held }, session, false)).toBeUndefined();
    expect(heldMessage({ key, subagent: null, held }, session, true)).toBeUndefined();
    expect(heldMessage({ key, subagent: null, held }, { ...session, archiveId: "a1" } as Session, false)).toBeUndefined();
  });

  it("stands for its own Sending row, and only that row", () => {
    const rows = [row("send-1", held.text), row("other", "An earlier message")];
    expect(withoutHeld(rows, held).map((r) => r.id)).toEqual(["other"]);
    expect(withoutHeld(rows, null)).toBe(rows);
    // The transcript then shows the held text once: in the held entry, not as "Sending…".
    const shown = JSON.stringify(buildTranscript([], {}, withoutHeld(rows, held)));
    expect(shown).not.toContain(held.text);
    expect(shown).toContain("An earlier message");
  });
});
