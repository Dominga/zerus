import { describe, expect, it } from "vitest";
import { jumpButton, searchHitItems } from "./search-hit";
import type { SearchHit } from "../../store/content-search-model";
import type { ActivityItem } from "./types";

// zerus-5wmh.13.17.5 (ActivityView.cpp:1592-1636): a shown search hit renders
// as Activity renders the message, and the jump button reads Back to latest
// activity while it shows.
const hit = (event: SearchHit["event"], truncated = false): SearchHit => ({ name: "codex/demo/api", run_id: "r", conversation_id: "c", archive_id: "", message_id: "m",
  at: 1_793_000_000, role: "assistant", snippet: "", event, content_truncated: truncated, session: null, group: "" });

describe("a shown search hit", () => {
  it("is the message as Activity shows it: Markdown for a reply, with Copy unless it is only an excerpt", () => {
    const [reply] = searchHitItems(hit({ seq: 1, type: "AgentMessage", at: 1_793_000_000, detail: "Use **needle** here" })) as ActivityItem[];
    expect(reply).toMatchObject({ kind: "agent", body: "Use **needle** here" });
    expect(reply.excerpt).toBeFalsy();
    const [excerpt] = searchHitItems(hit({ seq: 1, type: "AgentMessage", at: 1_793_000_000, detail: "…needle…" }, true)) as ActivityItem[];
    expect(excerpt).toMatchObject({ kind: "agent", excerpt: true });
    const [prompt] = searchHitItems(hit({ seq: 1, type: "UserPromptSubmit", at: 1_793_000_000, detail: "find the needle" })) as ActivityItem[];
    expect(prompt).toMatchObject({ kind: "user", body: "find the needle" });
    const [tool] = searchHitItems(hit({ seq: 1, type: "PostToolUse", tool: "Bash", at: 1_793_000_000, detail: "needle output" })) as ActivityItem[];
    expect(tool.kind).toBe("tool");
  });

  it("keeps the jump button as Back to latest activity while a search hit shows, even when following", () => {
    expect(jumpButton({ following: true, unseen: 0, searchShown: false })).toEqual({ visible: false, caption: "Jump to latest activity" });
    expect(jumpButton({ following: false, unseen: 3, searchShown: false })).toEqual({ visible: true, caption: "Jump to latest activity (3 new)" });
    expect(jumpButton({ following: true, unseen: 0, searchShown: true })).toEqual({ visible: true, caption: "Back to latest activity" });
    expect(jumpButton({ following: false, unseen: 2, searchShown: true })).toEqual({ visible: true, caption: "Back to latest activity" });
  });
});
