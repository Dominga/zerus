import { describe, expect, it } from "vitest";
import { toSession } from "../../store/catalog";
import {
  copyState, detailsFromInspection, dialogReducer, menuItemActivates, menuState, renameParts, renameProblem, snapshotOf, tagProblem, terminal, terminateCopy,
} from "./actions-ui";
import fixture from "../../../core/tests/fixtures/action-rows.json";
import type { Details, ViewRow } from "../../types";

const row = (extra: Partial<ViewRow> = {}, computer = "") => toSession(computer, "run:a", {
  name: "claude/proj/a", agent: "claude", state: "running", resumable: true, activity: "idle", process_state: "running", run_id: "r1",
  conversation_id: "c1", machine: "box", ...extra,
});
const details = (extra: Partial<Details> = {}): Details => ({
  run_id: "r1", conversation_id: "c1", auth_required: false, expected_id: "", resume_message_can_send: true, allowed_actions: null, action_reasons: {},
  interrupt_supported: false, turn_started: 0, phase: "", ...extra,
});

describe("menuState change", () => {
  it("pauses an idle running claude", () => {
    expect(menuState(row(), true, false).change).toMatchObject({ label: "Pause session", enabled: true });
  });
  it("does not pause a working one", () => expect(menuState(row({ activity: "busy" }), true, false).change.enabled).toBe(false));
  it("does not pause an exited process", () => expect(menuState(row({ process_state: "exited" }), true, false).change.enabled).toBe(false));
  it("resumes a paused resumable session", () => {
    expect(menuState(row({ state: "paused" }), true, false).change).toMatchObject({ label: "Resume session", enabled: true });
    expect(menuState(row({ state: "paused", resumable: false }), true, false).change.enabled).toBe(false);
  });
  it("is Updating and disabled while updating or offline", () => {
    expect(menuState(row(), true, true).change).toMatchObject({ label: "Updating…", enabled: false });
    expect(menuState(row(), false, false).change.enabled).toBe(false);
  });
  it("shows Restore disabled for an archived row", () => {
    expect(menuState(row({ state: "archived", archive_id: "a1" }), true, false).change).toMatchObject({ label: "Restore session", enabled: false });
  });
  it("lets auth_required stand in for idle with details", () => {
    expect(menuState(row({ activity: "busy" }), true, false, details({ auth_required: true })).change.enabled).toBe(true);
  });
  it("needs the resume message when an expected id is set", () => {
    expect(menuState(row(), true, false, details({ expected_id: "x", resume_message_can_send: false })).change.enabled).toBe(false);
    expect(menuState(row(), true, false, details({ expected_id: "x", resume_message_can_send: true })).change.enabled).toBe(true);
  });
  it("follows reported allowed actions, details first", () => {
    expect(menuState(row({ allowed_actions: ["rename"] }), true, false).change.enabled).toBe(false);
    expect(menuState(row({ allowed_actions: ["rename"] }), true, false, details({ allowed_actions: ["pause"] })).change.enabled).toBe(true);
    expect(menuState(row({ allowed_actions: [] }), true, false).rename).toBe(false);
  });
  it("lets an opencode session only rename and forget", () => {
    const m = menuState(row({ agent: "opencode" }), true, false);
    expect(m.change.enabled).toBe(false);
    expect(m.rename).toBe(true);
    expect(m.end.label).toBe("Forget saved session…");
  });
});

describe("menuState rename, archive, end, read", () => {
  it("renames with an agent and a project", () => {
    expect(menuState(row(), true, false).rename).toBe(true);
    expect(menuState(row({ name: "claude" }), true, false).rename).toBe(false);
    expect(menuState(row({ name: "/a" }), true, false).rename).toBe(false);
    expect(menuState(row(), false, false).rename).toBe(false);
  });
  it("renames an archived row only with its archive id", () => {
    expect(menuState(row({ state: "archived" }), true, false).rename).toBe(false);
    expect(menuState(row({ state: "archived", archive_id: "a1" }), true, false).rename).toBe(true);
  });
  it("offers archive for paused or stopped, not DeepSeek", () => {
    expect(menuState(row({ state: "paused" }), true, false).archive).toMatchObject({ visible: true, enabled: true });
    expect(menuState(row({ state: "stopped" }), true, false).archive.visible).toBe(true);
    expect(menuState(row(), true, false).archive.visible).toBe(false);
    // The core names DeepSeek's `dsh` sessions `deepseek`.
    expect(menuState(row({ state: "paused", agent: "deepseek" }), true, false).archive.visible).toBe(false);
  });
  it("needs the machine online for Move to archive in the context menu only", () => {
    expect(menuState(row({ state: "paused" }), false, false).archive.enabled).toBe(true);
    expect(menuState(row({ state: "paused" }), false, false, undefined, { context: true }).archive.enabled).toBe(false);
    expect(menuState(row({ state: "paused" }), true, false, undefined, { context: true }).archive.enabled).toBe(true);
  });
  it("passes the archive reason, details first", () => {
    expect(menuState(row({ state: "paused", allowed_actions: [], action_reasons: { archive: "No." } }), true, false).archive).toMatchObject({ enabled: false, reason: "No." });
    const m = menuState(row({ state: "paused", allowed_actions: [], action_reasons: { archive: "row" } }), true, false, details({ allowed_actions: [], action_reasons: { archive: "detail" } }));
    expect(m.archive.reason).toBe("detail");
  });
  it("terminates a running session", () => {
    expect(menuState(row(), true, false).end).toMatchObject({ label: "Terminate session…", enabled: true });
    expect(menuState(row(), false, false).end.enabled).toBe(false);
    expect(menuState(row({ allowed_actions: ["pause"], action_reasons: { terminate: "Not now" } }), true, false).end).toMatchObject({ enabled: false, reason: "Not now" });
  });
  it("forgets a running opencode binding and keeps forgets disabled", () => {
    expect(menuState(row({ agent: "opencode" }), true, false).end).toMatchObject({ label: "Forget saved session…", enabled: false, reason: "Forgetting sessions arrives in a later version." });
    expect(menuState(row({ state: "paused" }), true, false).end.label).toBe("Forget saved session…");
    expect(menuState(row({ state: "archived" }), true, false).end.label).toBe("Forget archived session…");
  });
  it("marks read when attention needs it and the host shares read state", () => {
    expect(menuState(row({ unread: true, shared_read: true }), true, false).markRead).toEqual({ label: "Mark as read", enabled: true });
    // A read mark runs in its own lane: a pending write does not hold it up.
    expect(menuState(row({ unread: true, shared_read: true }), true, true).markRead.enabled).toBe(true);
    expect(menuState(row({ unread: true, shared_read: true }), false, false).markRead.enabled).toBe(false);
    expect(menuState(row({ unread: true, shared_read: true }), true, false, undefined, { reading: true }).markRead.enabled).toBe(false);
    expect(menuState(row({ status: "attention", attention: "approval", shared_read: false }), true, false).markRead)
      .toMatchObject({ label: "Mark as read", enabled: false, reason: "Update hgs on this session's host to share read state." });
    expect(menuState(row(), true, false).markRead).toMatchObject({ label: "Mark needs attention", enabled: false, reason: "Arrives with attention marks." });
  });
});

describe("menuState stop", () => {
  const working = { phase: "tool", activity: "busy" } as const;
  it("is offered while the turn works and enabled when the host can interrupt it", () => {
    expect(menuState(row(working), true, false, details({ interrupt_supported: true })).stop).toEqual({ visible: true, enabled: true, reason: undefined });
  });
  it("stays off without details, since hosts never list interrupt as an allowed action", () => {
    expect(menuState(row({ ...working, allowed_actions: ["rename", "terminate", "forget"] }), true, false).stop)
      .toEqual({ visible: true, enabled: false, reason: "Interrupt is unavailable. Open Terminal to stop this turn." });
  });
  it("gives Qt's reasons when it is off", () => {
    expect(menuState(row(working), false, false, details({ interrupt_supported: true })).stop).toMatchObject({ enabled: false, reason: "Machine is offline" });
    expect(menuState(row(working), true, true, details({ interrupt_supported: true }), { interrupting: true }).stop)
      .toMatchObject({ enabled: false, reason: "Interrupt is being sent…" });
  });
  it("waits only for its own interrupt, not for another write (Qt's m_interruptRequest)", () => {
    expect(menuState(row(working), true, true, details({ interrupt_supported: true })).stop.enabled).toBe(true);
  });
  it("follows the inspected phase and a running state", () => {
    expect(menuState(row(), true, false, details({ interrupt_supported: true, phase: "compacting" })).stop.enabled).toBe(true);
    expect(menuState(row(working), true, false, details({ interrupt_supported: true, phase: "idle" })).stop.visible).toBe(false);
    expect(menuState(row({ ...working, state: "paused" }), true, false, details({ interrupt_supported: true })).stop.visible).toBe(false);
  });
});

describe("terminateCopy", () => {
  it("stops a tracked claude into Archive", () => {
    expect(terminateCopy(row({ tracked: true }))).toEqual({
      title: "Terminate session?",
      text: "Stop claude/proj/a and move its recorded conversation to Archive? Running processes will stop; the conversation history is kept.",
      detail: "Computer: box", confirm: "Stop and archive", action: "terminate",
    });
  });
  it("kills any other live session", () => {
    const c = terminateCopy(row({ tracked: false }));
    expect(c.text).toBe("Terminate claude/proj/a? Its running processes will stop. No HGS archive is available for this session; any native conversation history is kept.");
    expect(c.confirm).toBe("Terminate session");
    expect(c.action).toBe("kill");
    expect(terminateCopy(row({ tracked: true, agent: "opencode" })).action).toBe("kill");
    expect(terminateCopy(row({ tracked: true, agent: "deepseek" })).action).toBe("kill");
  });
});

describe("rename", () => {
  it("edits only the tag after the agent and project", () => {
    expect(renameParts("claude/proj/a")).toEqual({ prefix: "claude/proj/", tag: "a" });
    expect(renameParts("claude/proj")).toEqual({ prefix: "claude/proj/", tag: "" });
  });
  it("follows SessionTag::problem", () => {
    expect(tagProblem("release planning")).toBeUndefined();
    expect(tagProblem("")).toBe("Enter a session name.");
    for (const t of [" a", "a ", "\ta"]) expect(tagProblem(t)).toBe("Remove spaces at the beginning or end.");
    for (const t of ["a/b", "a:b", "v1.2", "a\u0007b"]) expect(tagProblem(t)).toBe("Names cannot contain /, : or . or control characters.");
  });
  it("refuses a live name already used on the same computer", () => {
    const s = row();
    const other = (name: string, key: string, extra: Partial<ViewRow> = {}, computer = "") => ({ ...row({ name, ...extra }, computer), key });
    const others = [other("claude/proj/b", "\nrun:b"), other("claude/proj/c", "laptop\nrun:c", {}, "laptop"), other("claude/proj/d", "\nrun:d", { state: "archived" })];
    expect(renameProblem("b", s, others)).toBe("This name is already in use on box.");
    expect(renameProblem("c", s, others)).toBeUndefined();
    expect(renameProblem("d", s, others)).toBeUndefined();
    expect(renameProblem("a", s, [s])).toBeUndefined();
    expect(renameProblem("b", row({ state: "archived", archive_id: "a1" }), others)).toBeUndefined();
    expect(renameProblem("b.c", s, others)).toBe("Names cannot contain /, : or . or control characters.");
  });
});

describe("details", () => {
  it("reads the inspection fields of an inspect reply or a state value", () => {
    expect(detailsFromInspection({ run_id: "r", conversation_id: "c", phase: "tool", turn_started: 1.5, interrupt_supported: true, allowed_actions: ["pause", 3], action_reasons: { resume: "busy", x: 1 } }))
      .toEqual({ run_id: "r", conversation_id: "c", auth_required: false, expected_id: "", resume_message_can_send: false, allowed_actions: ["pause"],
        action_reasons: { resume: "busy" }, interrupt_supported: true, turn_started: 1.5, phase: "tool", cwd: "", terminal_supported: null, terminal_attach_supported: null });
    expect(detailsFromInspection(null)).toBeUndefined();
    expect(detailsFromInspection([1])).toBeUndefined();
  });
});

describe("dialog helpers", () => {
  it("opens one dialog at a time", () => {
    const a = snapshotOf(row());
    let state = dialogReducer(undefined, { type: "open", kind: "rename", row: a });
    expect(state?.kind).toBe("rename");
    state = dialogReducer(state, { type: "open", kind: "terminate", row: a });
    expect(state).toEqual({ kind: "terminate", row: a });
    expect(dialogReducer(state, { type: "close" })).toBeUndefined();
  });
  it("ignores disabled menu items", () => {
    expect(menuItemActivates({ disabled: true })).toBe(false);
    expect(menuItemActivates({})).toBe(true);
  });
  it("snapshots plain values", () => {
    const s = row({ reply_id: "r1" });
    const snap = snapshotOf(s);
    expect(snap).toMatchObject({ key: s.key, replyId: "r1", runId: "r1", cmd: "claude" });
    expect(Object.keys(snap)).not.toContain("title");
  });
});

// The rows the core projects from real hgs listings (core/tests/fixtures,
// checked against view.rs), so these menus see the values hgs sends.
describe("menus from hgs rows as the core projects them", () => {
  const cases = Object.fromEntries(fixture.rows.map((c) => [c.case, c]));
  const projected = (name: string) => {
    const c = cases[name];
    return { s: toSession(c.computer, c.key, c.view as ViewRow), state: "state" in c ? c.state : undefined };
  };

  it("stops a working Claude turn through its inspected details, never through allowed_actions", () => {
    const { s, state } = projected("a working tracked Claude turn");
    expect(s.allowed?.split(" ")).not.toContain("interrupt");
    const d = detailsFromInspection(state)!;
    expect(d).toMatchObject({ run_id: s.runId, conversation_id: s.conversationId, interrupt_supported: true, turn_started: 1793000000.5 });
    const m = menuState(s, true, false, d);
    expect(m.stop).toEqual({ visible: true, enabled: true, reason: undefined });
    expect(menuState(s, true, false).stop.enabled).toBe(false);
    expect(m.change).toMatchObject({ label: "Pause session", enabled: false });
    expect(m.end).toMatchObject({ label: "Terminate session…", enabled: true });
    expect(m.rename).toBe(true);
    expect(terminateCopy(s).action).toBe("terminate");
  });

  it("kills a live session hgs does not track", () => {
    const { s } = projected("a live session hgs does not track");
    expect(s.state).toBe("running");
    expect(s.runId).toBeUndefined();
    const m = menuState(s, true, false, undefined, { context: true });
    expect(m.end).toMatchObject({ label: "Terminate session…", enabled: true });
    expect(m.rename).toBe(true);
    expect(m.stop.visible).toBe(false);
    expect(terminateCopy(s)).toMatchObject({ action: "kill", confirm: "Terminate session" });
  });

  it("never offers archive for DeepSeek and resumes it as reported", () => {
    const { s } = projected("a paused DeepSeek session");
    expect(s.cmd).toBe("deepseek");
    const m = menuState(s, true, false, undefined, { context: true });
    expect(m.archive.visible).toBe(false);
    expect(m.change).toMatchObject({ label: "Resume session", enabled: true });
    expect(m.end).toMatchObject({ label: "Forget saved session…", enabled: false });
  });

  it("forgets only the binding of an OpenCode session", () => {
    const { s } = projected("an idle OpenCode session");
    expect(s.cmd).toBe("opencode");
    const m = menuState(s, true, false);
    expect(m.change.enabled).toBe(false);
    expect(m.rename).toBe(true);
    expect(m.end).toMatchObject({ label: "Forget saved session…", enabled: false, reason: "Forgetting sessions arrives in a later version." });
  });
});

describe("terminal", () => {
  it("is never offered for deepseek", () => {
    expect(terminal(row({ agent: "deepseek", terminal_supported: true, terminal_reported: true }), details({ terminal_supported: true }))).toBe(false);
    expect(terminal(row({ agent: "dsh", terminal_supported: true }))).toBe(false);
  });
  it("follows the row for other agents and lets the details win", () => {
    expect(terminal(row({ terminal_supported: true }))).toBe(true);
    expect(terminal(row({ terminal_supported: false }))).toBe(false);
    expect(terminal(row({ terminal_supported: false }), details({ terminal_supported: true }))).toBe(true);
    expect(terminal(row({ terminal_supported: true }), details({ terminal_supported: false }))).toBe(false);
    expect(terminal(row({ terminal_supported: true }), details({ terminal_supported: null }))).toBe(true);
  });
  it("reads the attach support first for opencode, details before the row", () => {
    const oc = (extra: Partial<ViewRow>) => row({ agent: "opencode", ...extra });
    expect(terminal(oc({ terminal_attach_supported: true, terminal_attach_reported: true }))).toBe(true);
    expect(terminal(oc({ terminal_attach_supported: true, terminal_attach_reported: true }), details({ terminal_attach_supported: false }))).toBe(false);
    expect(terminal(oc({ terminal_attach_supported: false, terminal_attach_reported: true, terminal_supported: true, terminal_reported: true }))).toBe(false);
    expect(terminal(oc({}), details({ terminal_attach_supported: true }))).toBe(true);
  });
  it("gives an opencode row without a reported value no terminal", () => {
    expect(terminal(row({ agent: "opencode", terminal_supported: true }))).toBe(false);
    expect(terminal(row({ agent: "opencode", terminal_supported: true, terminal_reported: true }))).toBe(true);
    expect(terminal(row({ agent: "opencode" }), details({ terminal_supported: true }))).toBe(true);
  });
});

describe("copyState", () => {
  it("offers connect for a terminal row and restore for an archived one", () => {
    expect(copyState(row({ terminal_supported: true }), true).command).toEqual({ label: "Copy connect command", enabled: true });
    expect(copyState(row({ terminal_supported: false }), true).command).toEqual({ label: "Copy connect command", enabled: false });
    expect(copyState(row({ state: "archived", archive_id: "a1", terminal_supported: false }), true).command).toEqual({ label: "Copy restore command", enabled: true });
  });
  it("needs an archive id for an archived row", () => {
    expect(copyState(row({ state: "archived", terminal_supported: true }), true).command.enabled).toBe(false);
  });
  it("copies the name always and the reference only online with a run id", () => {
    expect(copyState(row(), false).reference).toBe(false);
    expect(copyState(row({ run_id: undefined }), true).reference).toBe(false);
    expect(copyState(row(), true).reference).toBe(true);
  });
  it("copies the details folder first, else the row's, and is off when empty", () => {
    expect(copyState(row({ cwd: "/w/row" }), true).folder).toEqual({ enabled: true, path: "/w/row" });
    expect(copyState(row({ cwd: "/w/row" }), true, details({ cwd: "/w/detail" })).folder.path).toBe("/w/detail");
    expect(copyState(row({ cwd: "/w/row" }), true, details({ cwd: "" })).folder.path).toBe("/w/row");
    expect(copyState(row(), true).folder).toEqual({ enabled: false, path: "" });
  });
  it("reads the inspection cwd and terminal flags", () => {
    expect(detailsFromInspection({ cwd: "/w/x", terminal_supported: false, terminal_attach_supported: true }))
      .toMatchObject({ cwd: "/w/x", terminal_supported: false, terminal_attach_supported: true });
  });
});
