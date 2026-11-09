import { describe, expect, it } from "vitest";
import { NOT_DELIVERED_HINT, REMOVED, REMOVE_CHANGED, REMOVE_TEXT, REMOVE_TITLE, removeNotice, removeState } from "./inbox-actions";
import type { InboxItem } from "./types";

const item: InboxItem = { state: "not_delivered", messageId: "msg_queued01", run: "run-1", conversation: "conv-1", removable: true };

describe("Remove for a not-delivered OpenCode message (zerus-5wmh.13.6)", () => {
  it("keeps Qt's texts", () => {
    expect([REMOVE_TITLE, REMOVE_TEXT, NOT_DELIVERED_HINT, REMOVE_CHANGED, REMOVED]).toEqual([
      "Remove queued message",
      "Remove this queued message? It is still waiting in OpenCode's queue and runs first with your next message unless you remove it.",
      "Still waiting in OpenCode's queue. It runs first with your next message unless you remove it.",
      "The queued message changed. Check Activity before removing it.",
      "Queued message removed.",
    ]);
  });

  it("is off while any removal runs, and names its own", () => {
    expect(removeState(item, { inbox_removing: null, inbox_busy: false })).toEqual({ label: "Remove", disabled: false });
    expect(removeState(item, undefined)).toEqual({ label: "Remove", disabled: false });
    expect(removeState(item, { inbox_removing: "msg_queued01", inbox_busy: true })).toEqual({ label: "Removing…", disabled: true });
    // Another session's removal runs: one at a time across sessions.
    expect(removeState(item, { inbox_removing: null, inbox_busy: true })).toEqual({ label: "Remove", disabled: true });
  });

  it("words each outcome once; a busy Remove ran nothing and says nothing", () => {
    expect(removeNotice({ outcome: "removed" })).toEqual({ tone: "info", text: "Queued message removed.", sticky: false });
    expect(removeNotice({ outcome: "changed" })).toEqual({ tone: "error", text: REMOVE_CHANGED, sticky: true });
    expect(removeNotice({ outcome: "busy" })).toBeUndefined();
    expect(removeNotice({ outcome: "failed", detail: "OpenCode could not confirm this message is still queued" }))
      .toEqual({ tone: "error", text: "OpenCode could not confirm this message is still queued", sticky: true });
    expect(removeNotice({ outcome: "uncertain", detail: "The result was not confirmed. Refresh before trying again." }))
      .toEqual({ tone: "error", text: "The result was not confirmed. Refresh before trying again.", sticky: true });
  });
});
