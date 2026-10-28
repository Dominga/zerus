import { describe, expect, it } from "vitest";
import { menuState, renameError, terminateCopy } from "./actions-ui";
import type { Details, RowOut, ViewRow } from "../../types";

const entry = (extra: Partial<ViewRow> = {}): RowOut => ({
  computer: "", key: "run:a",
  row: { name: "claude/proj/a", agent: "claude", state: "running", resumable: true, activity: "idle", process_state: "running", run_id: "r1", conversation_id: "c1", machine: "box", ...extra },
});
const details = (extra: Partial<Details> = {}): Details => ({
  run_id: "r1", conversation_id: "c1", auth_required: false, expected_id: "", resume_message_can_send: true, allowed_actions: null, action_reasons: {}, ...extra,
});

describe("menuState change", () => {
  it("pauses an idle running claude", () => {
    const m = menuState(entry(), true, false);
    expect(m.change).toMatchObject({ label: "Pause session", enabled: true });
  });
  it("does not pause a working one", () => expect(menuState(entry({ activity: "working" }), true, false).change.enabled).toBe(false));
  it("does not pause an exited process", () => expect(menuState(entry({ process_state: "exited" }), true, false).change.enabled).toBe(false));
  it("resumes a paused resumable session", () => {
    const m = menuState(entry({ state: "paused" }), true, false);
    expect(m.change).toMatchObject({ label: "Resume session", enabled: true });
    expect(menuState(entry({ state: "paused", resumable: false }), true, false).change.enabled).toBe(false);
  });
  it("is Updating and disabled while updating or offline", () => {
    expect(menuState(entry(), true, true).change).toMatchObject({ label: "Updating…", enabled: false });
    expect(menuState(entry(), false, false).change.enabled).toBe(false);
  });
  it("shows Restore disabled for an archived row", () => {
    const m = menuState(entry({ state: "archived", archive_id: "a1" }), true, false);
    expect(m.change).toMatchObject({ label: "Restore session", enabled: false });
  });
  it("lets auth_required stand in for idle with details", () => {
    expect(menuState(entry({ activity: "working" }), true, false, details({ auth_required: true })).change.enabled).toBe(true);
  });
  it("needs the resume message when an expected id is set", () => {
    expect(menuState(entry(), true, false, details({ expected_id: "x", resume_message_can_send: false })).change.enabled).toBe(false);
    expect(menuState(entry(), true, false, details({ expected_id: "x", resume_message_can_send: true })).change.enabled).toBe(true);
  });
  it("follows reported allowed actions, details first", () => {
    expect(menuState(entry({ allowed_actions: ["rename"] }), true, false).change.enabled).toBe(false);
    expect(menuState(entry({ allowed_actions: ["rename"] }), true, false, details({ allowed_actions: ["pause"] })).change.enabled).toBe(true);
  });
  it("lets an opencode session only rename and forget", () => {
    const m = menuState(entry({ agent: "opencode" }), true, false);
    expect(m.change.enabled).toBe(false);
    expect(m.rename).toBe(true);
    expect(m.end.label).toBe("Forget saved session…");
  });
});

describe("menuState rename, archive, end, stop, read", () => {
  it("renames with both name parts", () => {
    expect(menuState(entry(), true, false).rename).toBe(true);
    expect(menuState(entry({ name: "claude" }), true, false).rename).toBe(false);
    expect(menuState(entry({ name: "/a" }), true, false).rename).toBe(false);
    expect(menuState(entry(), false, false).rename).toBe(false);
  });
  it("renames an archived row only with its archive id", () => {
    expect(menuState(entry({ state: "archived" }), true, false).rename).toBe(false);
    expect(menuState(entry({ state: "archived", archive_id: "a1" }), true, false).rename).toBe(true);
  });
  it("offers archive for paused or stopped, not dsh", () => {
    expect(menuState(entry({ state: "paused" }), true, false).archive).toMatchObject({ visible: true, enabled: true });
    expect(menuState(entry({ state: "stopped" }), true, false).archive.visible).toBe(true);
    expect(menuState(entry(), true, false).archive.visible).toBe(false);
    expect(menuState(entry({ state: "paused", agent: "dsh" }), true, false).archive.visible).toBe(false);
  });
  it("passes the archive reason", () => {
    const m = menuState(entry({ state: "paused", allowed_actions: [], action_reasons: { archive: "No." } }), true, false);
    expect(m.archive).toMatchObject({ enabled: false, reason: "No." });
  });
  it("terminates a running session", () => {
    expect(menuState(entry(), true, false).end).toMatchObject({ label: "Terminate session…", enabled: true });
    expect(menuState(entry(), false, false).end.enabled).toBe(false);
    expect(menuState(entry({ allowed_actions: ["pause"] }), true, false).end.enabled).toBe(false);
  });
  it("forgets a running opencode binding and keeps forgets disabled", () => {
    const m = menuState(entry({ agent: "opencode" }), true, false);
    expect(m.end).toMatchObject({ label: "Forget saved session…", enabled: false, reason: "Forgetting sessions arrives in a later version." });
    expect(menuState(entry({ state: "paused" }), true, false).end.label).toBe("Forget saved session…");
    expect(menuState(entry({ state: "archived" }), true, false).end.label).toBe("Forget archived session…");
  });
  it("stops only a started turn", () => {
    expect(menuState(entry({ turn_started: 100 }), true, false).stop).toBe(true);
    expect(menuState(entry(), true, false).stop).toBe(false);
    expect(menuState(entry({ turn_started: 100 }), true, true).stop).toBe(false);
    expect(menuState(entry({ turn_started: 100, allowed_actions: [] }), true, false).stop).toBe(false);
  });
  it("marks read when attention needs it and the host shares read state", () => {
    expect(menuState(entry({ unread: true, shared_read: true }), true, false).markRead).toEqual({ label: "Mark as read", enabled: true });
    expect(menuState(entry({ attention: "approval", shared_read: false }), true, false).markRead)
      .toMatchObject({ label: "Mark as read", enabled: false, reason: "Update hgs on this session's host to share read state." });
    expect(menuState(entry(), true, false).markRead)
      .toMatchObject({ label: "Mark needs attention", enabled: false, reason: "Arrives with attention marks." });
  });
});

describe("terminateCopy", () => {
  it("archives a tracked claude", () => {
    expect(terminateCopy(entry({ tracked: true }))).toEqual({
      title: "Terminate session?",
      text: "Stop claude/proj/a and move its recorded conversation to Archive? Running processes will stop; the conversation history is kept.",
      detail: "Computer: box", confirm: "Stop and archive",
    });
  });
  it("has no archive otherwise", () => {
    const c = terminateCopy(entry({ tracked: false }));
    expect(c.text).toBe("Terminate claude/proj/a? Its running processes will stop. No HGS archive is available for this session; any native conversation history is kept.");
    expect(c.confirm).toBe("Terminate session");
    expect(terminateCopy(entry({ tracked: true, agent: "opencode" })).confirm).toBe("Terminate session");
  });
});

describe("renameError", () => {
  it("accepts a project and a session part", () => expect(renameError("proj/a")).toBeUndefined());
  it("refuses the rest", () => {
    for (const n of ["", "proj", "/a", "proj/", "a b/c", "a/b c", "a=b/c", "#a/b"]) expect(renameError(n)).toMatch(/^Use a name without spaces/);
  });
});
