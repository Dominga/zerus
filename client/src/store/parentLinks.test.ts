import { describe, expect, it, vi } from "vitest";
import { toSession } from "./catalog";
import { candidatesAmong, nestable } from "./parentLinks";
import type { ViewRow } from "../types";

// The list model's module reads the page's query (store/ui.ts); the tests run without a page.
vi.hoisted(() => { vi.stubGlobal("location", new URL("http://localhost/")); });

const MACHINE = "11111111-1111-4111-8111-111111111111";
const id = (n: number) => `00000000-0000-4000-8000-${String(n).padStart(12, "0")}`;
const session = (n: number, parent?: number, extra: Partial<ViewRow> = {}, computer = "") =>
  toSession(computer, `run:${n}`, { name: `claude/p/s${n}`, agent: "claude", status: "ready", session_id: id(n), run_id: `run-${n}`,
    conversation_id: `c${n}`, parent: parent === undefined ? undefined
      : { machine: MACHINE, session_id: id(parent), name: `claude/p/s${parent}`, conversation_id: `c${parent}`, run_id: `run-${parent}`, state: "live" },
    ...extra });

describe("parent links on the desktop (zerus-sa9qn.9)", () => {
  it("accepts identified sessions of every supported agent in both directions", () => {
    for (const child of ["claude", "codex", "kimi", "dsh", "opencode"]) {
      for (const parent of ["claude", "codex", "kimi", "dsh", "opencode"]) {
        expect(nestable(session(1, undefined, { agent: child }), session(2, undefined, { agent: parent }))).toBe(true);
      }
    }
  });

  it("requires complete identities, one computer and different unarchived sessions", () => {
    expect(nestable(session(1), session(2))).toBe(true);
    expect(nestable(session(1), session(1))).toBe(false);
    expect(nestable(session(1), session(2, undefined, {}, "peer"))).toBe(false);
    expect(nestable(session(1), session(2, undefined, { session_id: undefined }))).toBe(false);
    expect(nestable(session(1), session(2, undefined, { agent: "sh" }))).toBe(false);
    expect(nestable(session(1, undefined, { agent: "dsh", run_id: undefined }), session(2))).toBe(false);
    expect(nestable(session(1), session(2, undefined, { agent: "opencode", conversation_id: undefined }))).toBe(false);
    expect(nestable(session(1), session(2, undefined, { state: "archived" }))).toBe(false);
  });

  it("excludes descendants through mixed native and terminal parents", () => {
    const rows = [session(1, undefined, { agent: "dsh" }), session(2, 1, { agent: "opencode" }),
      session(3, 2), session(4, undefined, { agent: "opencode" })];
    expect(candidatesAmong(rows[0], rows, () => MACHINE).map((s) => s.sessionId)).toEqual([id(4)]);
  });

  it("never offers the child itself or its own descendants as its parent", () => {
    const rows = [session(1), session(2, 1), session(3, 2), session(4), session(5, undefined, {}, "peer")];
    const offered = candidatesAmong(rows[0], rows, (computer) => (computer === "" ? MACHINE : undefined)).map((s) => s.name);
    expect(offered).toEqual(["claude/p/s4"]);
    expect(candidatesAmong(rows[2], rows, () => MACHINE).map((s) => s.name)).toEqual(["claude/p/s1", "claude/p/s2", "claude/p/s4"]);
  });
});
