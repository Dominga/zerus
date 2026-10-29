import { describe, expect, it } from "vitest";
import { actionNeeded, headerAttention, isActive, isWorking, needsAttention } from "./attention";
import type { Session } from "../ui/sessions/types";

const row = (extra: Partial<Session> = {}): Session =>
  ({ state: "running", status: "ready", unread: false, draft: false, ...extra }) as Session;

describe("the window's online rule (SessionPresentation.h)", () => {
  it("counts work and background jobs only while online (working, 268-271)", () => {
    for (const status of ["working", "background"] as const) {
      expect(isWorking(row({ status }), true)).toBe(true);
      expect(isWorking(row({ status }), false)).toBe(false);
    }
    expect(isWorking(row({ status: "scheduled" }), true)).toBe(false);
  });

  it("keeps a computer that is not online active only for Review later or a draft (active, 308-312)", () => {
    for (const status of ["working", "background", "scheduled", "waiting"] as const) {
      expect(isActive(row({ status }), true)).toBe(true);
      expect(isActive(row({ status }), false)).toBe(false);
    }
    expect(isActive(row({ status: "attention", attentionNeeded: true }), false)).toBe(false);
    expect(isActive(row({ reviewLater: true }), false)).toBe(true);
    expect(isActive(row({ draft: true }), false)).toBe(true);
  });

  it("a header's attention leaves the unread reply to the unread counter (SessionsWindowRows.cpp:400-401)", () => {
    const reply = row({ unread: true, attentionNeeded: true, actionNeeded: false });
    expect(needsAttention(reply, true)).toBe(true);
    expect(headerAttention(reply, true)).toBe(false);
    const asking = row({ status: "attention", attentionNeeded: true, actionNeeded: true });
    expect(headerAttention(asking, true)).toBe(true);
    expect(headerAttention(asking, false)).toBe(false);
    expect(headerAttention(row({ childAttention: true }), true)).toBe(true);
    expect(headerAttention(row({ childAttention: true }), false)).toBe(false);
    expect(headerAttention(row({ reviewLater: true }), false)).toBe(true);
    expect(headerAttention(row({ reviewLater: true, state: "archived" }), true)).toBe(false);
  });

  it("an acknowledged request is not a request; a row without the overlay reads its status", () => {
    expect(actionNeeded(row({ status: "ready", actionNeeded: false }))).toBe(false);
    expect(actionNeeded(row({ status: "attention" }))).toBe(true);
    expect(actionNeeded(row({ status: "error" }))).toBe(false);
  });
});
