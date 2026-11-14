import { expect, it } from "vitest";
import { leadChatBound } from "./lead-chat";
import { draftKey } from "../../store/draftKeys";
import { teamView } from "./fixtures";

it("handwritten lead chat controls require the bound machine, session, live run, conversation and composer", () => {
  const row = { computer: "", catalogKey: "run:current", name: "codex/example/lead", sessionId: "11111111-1111-4111-8111-111111111111", state: "live", runId: "current", launchId: "launch", conversationId: "conversation" };
  const key = draftKey({ computer: "", key: row.catalogKey, name: row.name, session_id: row.sessionId, archive_id: null });
  const view = { ...teamView, members: teamView.members.map((member) => member.lead ? { ...member, session: { machine: "machine", session_id: "launch_id:launch" } } : member) };
  expect(leadChatBound(view, row, "machine", key, false)).toBe(true);
  for (const args of [
    [view, row, "other", key, false], [view, { ...row, launchId: "other" }, "machine", key, false],
    [view, { ...row, conversationId: "" }, "machine", key, false], [view, { ...row, runId: "" }, "machine", key, false],
    [view, { ...row, state: "archived" }, "machine", key, false], [view, row, "machine", "old-key", false], [view, row, "machine", key, true],
    [view, undefined, "machine", key, false],
  ] as const) expect(leadChatBound(args[0], args[1], args[2], args[3], args[4])).toBe(false);
});
