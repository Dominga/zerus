import { describe, expect, it } from "vitest";
import { newChildren, parseSeen, type Seen } from "./childRows";
import type { ParentLinkOut } from "../types";

const A = "11111111-1111-4111-8111-111111111111";
const B = "33333333-3333-4333-8333-333333333333";
const parent = (at: number, machine = A): ParentLinkOut =>
  ({ machine, session_id: "22222222-2222-4222-8222-222222222222", name: "claude/p/lead", conversation_id: "c", run_id: "r", state: "live", at });
const row = (key: string, at?: number, machine = A) => ({ key, sessionId: `id-${key}`, parent: at === undefined ? undefined : parent(at, machine) });

describe("new child rows start compact (zerus-sa9qn.14)", () => {
  it("records a computer's existing children without compacting any", () => {
    const first = newChildren([row("lead"), row("a", 100), row("b", 120)], {});
    expect(first.fresh).toEqual([]);
    expect(first.changed).toBe(true);
    expect(first.seen[A]).toEqual([120, ["id-b"]]);
  });

  it("compacts a child linked after the record, once", () => {
    const seen: Seen = { [A]: [120, ["id-b"]] };
    const rows = [row("a", 100), row("b", 120), row("c", 130)];
    const next = newChildren(rows, seen);
    expect(next.fresh).toEqual(["c"]);
    expect(next.seen[A]).toEqual([130, ["id-c"]]);
    // The next listing (or a restart with the saved record) changes nothing:
    // a row the person expanded since stays expanded.
    const again = newChildren(rows, parseSeen(JSON.stringify(next.seen))!);
    expect(again).toEqual({ fresh: [], seen: next.seen, changed: false });
  });

  it("keeps siblings linked in the same second apart", () => {
    const seen: Seen = { [A]: [130, ["id-c"]] };
    const next = newChildren([row("c", 130), row("d", 130)], seen);
    expect(next.fresh).toEqual(["d"]);
    expect(next.seen[A]).toEqual([130, ["id-c", "id-d"]]);
  });

  it("compares each link with the clock of the computer that stamped it", () => {
    // B's clock runs behind A's: its new child is still new.
    const seen: Seen = { [A]: [5000, ["id-x"]], [B]: [100, ["id-y"]] };
    const next = newChildren([row("x", 5000), row("y", 100, B), row("z", 200, B)], seen);
    expect(next.fresh).toEqual(["z"]);
    expect(next.seen[B]).toEqual([200, ["id-z"]]);
    expect(next.seen[A]).toEqual([5000, ["id-x"]]);
  });

  it("an adoption counts as new and an unlinked or undated row never does", () => {
    const seen: Seen = { [A]: [100, ["id-a"]] };
    expect(newChildren([row("a", 100), row("old", 150)], seen).fresh).toEqual(["old"]);
    expect(newChildren([row("plain"), { key: "undated", sessionId: "u", parent: parent(0) }], seen).changed).toBe(false);
  });

  it("reads only a well-formed record", () => {
    expect(parseSeen("")).toBeUndefined();
    expect(parseSeen("not json")).toBeUndefined();
    expect(parseSeen("[1,2]")).toBeUndefined();
    expect(parseSeen(JSON.stringify({ [A]: [1.5, []], [B]: [7, ["ok", 3]] }))).toEqual({ [B]: [7, ["ok"]] });
  });
});
