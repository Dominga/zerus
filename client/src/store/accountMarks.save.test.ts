import { beforeEach, describe, expect, it, vi } from "vitest";

// zerus-va84t: what saving an account's appearance writes for each member.
const writes = vi.hoisted(() => [] as [string, unknown][]);
vi.mock("./settings", async (original) => ({
  ...(await original<typeof import("./settings")>()),
  setSetting: vi.fn(async (key: string, value: unknown) => { writes.push([key, value]); return true; }),
}));

import { profileToken, saveAccountAppearance } from "./accountMarks";
import type { Account } from "../types";

const members = [{ host: "", id: "claude-a1" }, { host: "studio", id: "claude-b2" }];
// Only each member's host and profile ID take part in saving.
const account = { members } as unknown as Pick<Account, "members">;
const hideWrites = () => writes.filter(([key]) => key.startsWith("accounts/hide/"));
const hideKeys = members.map((m) => `accounts/hide/${profileToken(m.host, m.id)}`);

describe("saving an account's appearance", () => {
  beforeEach(() => { writes.length = 0; });

  it("Apply with the label hidden writes true for every member", async () => {
    expect(await saveAccountAppearance(account, "#2b97ff", null, true)).toBe(true);
    expect(hideWrites()).toEqual(hideKeys.map((key) => [key, true]));
  });

  it("Apply without hiding clears the flag for every member", async () => {
    expect(await saveAccountAppearance(account, "#2b97ff", "🦊", false)).toBe(true);
    expect(hideWrites()).toEqual(hideKeys.map((key) => [key, null]));
  });

  it("Reset to default clears color, mark and the hide flag for every member", async () => {
    // The dialog's Reset passes (null, null, false).
    expect(await saveAccountAppearance(account, null, null, false)).toBe(true);
    expect(writes.every(([, value]) => value === null)).toBe(true);
    expect(hideWrites().map(([key]) => key)).toEqual(hideKeys);
    expect(writes).toHaveLength(members.length * 3);
  });
});
