import { beforeEach, describe, expect, it, vi } from "vitest";

vi.hoisted(() => {
  const store = new Map<string, string>();
  vi.stubGlobal("location", new URL("http://localhost/"));
  vi.stubGlobal("localStorage", { getItem: (key: string) => store.get(key) ?? null, setItem: (key: string, value: string) => { store.set(key, value); }, removeItem: (key: string) => { store.delete(key); } });
  vi.stubGlobal("window", { document: { addEventListener: () => {} }, addEventListener: () => {} });
});
const mocks = vi.hoisted(() => ({ online: vi.fn() }));
vi.mock("../../store/catalog", async (orig) => ({
  ...(await orig<object>()),
  computerFresh: mocks.online,
  computerOffline: (computer: string) => !mocks.online(computer),
}));

import { badgeKind, emphasis } from "./SessionCard";
import type { Session } from "./types";

const row = (extra: Partial<Session> = {}): Session => ({
  computer: "", state: "running", status: "ready", unread: false, unreadReply: true, unreadCandidate: false, draft: false, ...extra,
}) as Session;

describe("session card unread presentation", () => {
  beforeEach(() => mocks.online.mockReset());

  it("shows the badge and edge on a full or compact row when the computer went offline mid-reply", () => {
    mocks.online.mockReturnValue(false);
    expect(badgeKind(row())).toBe("unread");
    expect(emphasis(row())).toBe("unread");
    expect(badgeKind(row({ muted: true }))).toBe("muted-unread");
  });

  it("keeps the same reply hidden while the computer is online", () => {
    mocks.online.mockReturnValue(true);
    expect(badgeKind(row())).toBe("neutral");
    expect(emphasis(row())).toBeUndefined();
  });
});
