import { describe, expect, it } from "vitest";
import { accountAppearance, accountMark, inkFor, labelSlot, markIndex, markLetter, profileToken, rowMark, validEmoji } from "./accountMarks";
import { usageOf } from "./testing";
import { emptyAccounts, type Account, type Profile } from "../types";

const US = String.fromCharCode(31);
const member = (extra: Partial<Profile> = {}): Profile => ({
  key: `${US}claude-a1`, host: "", machine: "workstation", id: "claude-a1", label: "Work", provider: "claude", native: false,
  installed: true, is_default: false, home: "/home/user/.config/hgs/accounts/claude-a1", auth_revision: "", online: true, state: "ready", detail: { source: "" },
  signed_in: true, permission: "auto", permission_label: { source: "Auto" }, permission_hint: { source: "" }, model_supported: true, model_text: { source: "" },
  usage: usageOf(), opencode: null, ...extra,
});
const account = (members: Profile[], key: string, label: string): Account => ({
  key, label, provider: members[0].provider, installed: true, machines: members.map((m) => m.machine), members,
  usage: usageOf(), permission: { source: "Auto" }, identity_lines: [], identity_note: null,
});
const row = (extra: { computer?: string; cmd?: string; accountId?: string; accountHome?: string; accountReported?: boolean } = {}) =>
  ({ computer: "", cmd: "claude", accountId: "claude-a1", accountHome: "", ...extra });

describe("account mark letters", () => {
  it("uses the first letter or digit of the label, uppercased", () => {
    expect(markLetter("work")).toBe("W");
    expect(markLetter("  (main) team")).toBe("M");
    expect(markLetter("42 lab")).toBe("4");
    expect(markLetter("ёлка")).toBe("Ё");
  });
  it("falls back to the provider's initial, then a question mark", () => {
    expect(markLetter("", "claude")).toBe("C");
    expect(markLetter("--", "")).toBe("?");
  });
});

describe("emoji marks", () => {
  it("accepts exactly one emoji grapheme, including flags and joined sequences", () => {
    expect(validEmoji("🦊")).toBe(true);
    expect(validEmoji(" 🦊 ")).toBe(true);
    expect(validEmoji("🇩🇪")).toBe(true);
    expect(validEmoji("👩‍💻")).toBe(true);
  });
  it("refuses letters, several emoji and empty text", () => {
    expect(validEmoji("A")).toBe(false);
    expect(validEmoji("🦊🐻")).toBe(false);
    expect(validEmoji("")).toBe(false);
    expect(validEmoji("🦊a")).toBe(false);
  });
});

describe("account colors", () => {
  it("picks the ink with the stronger contrast, including medium colors", () => {
    expect(inkFor("#ffcf33")).toBe("#10151b");
    expect(inkFor("#1f2a44")).toBe("#ffffff");
    // Medium swatches: dark ink gives about 6:1 where white gives about 3:1 (zerus-gjwsi).
    for (const medium of ["#2b97ff", "#00aa7f", "#f85c63", "#8796aa", "#6677ff"]) expect(inkFor(medium), medium).toBe("#10151b");
    expect(inkFor("#5a2ca0")).toBe("#ffffff");
  });
  it("derives a stable palette slot from the label when no color is chosen", () => {
    const work = account([member()], "a", "Work");
    expect(accountMark(work, {}).color).toBe(`var(--z-entity-palette-${labelSlot("Work")})`);
    expect(labelSlot("Work")).toBe(labelSlot("Work"));
  });
  it("reads the first member's chosen color and emoji, ignoring invalid values", () => {
    const one = member(), two = member({ key: `studio${US}claude-a1`, host: "studio", machine: "studio" });
    const values = {
      [`accounts/color/${profileToken("studio", "claude-a1")}`]: "#2B97FF",
      [`accounts/mark/${profileToken("", "claude-a1")}`]: "not an emoji",
      [`accounts/mark/${profileToken("studio", "claude-a1")}`]: "🦊",
    };
    expect(accountAppearance(values, [one, two])).toEqual({ color: "#2b97ff", emoji: "🦊", hidden: false });
  });
  it("prefers the computer's mark, with this desktop's own standing in until it moves or for an older hgs (zerus-sa9qn.6)", () => {
    const desktop = { [`accounts/color/${profileToken("", "claude-a1")}`]: "#ff9234", [`accounts/mark/${profileToken("", "claude-a1")}`]: "🐙" };
    const kept = member({ marks_supported: true, mark_color: "#2b97ff", mark_emoji: "🦊" });
    expect(accountAppearance(desktop, [kept])).toEqual({ color: "#2b97ff", emoji: "🦊", hidden: false });
    expect(accountAppearance(desktop, [member({ marks_supported: true, mark_color: "", mark_emoji: "" })])).toEqual({ color: "#ff9234", emoji: "🐙", hidden: false });
    // An older hgs reports nothing; its values are ignored if a client sends any.
    expect(accountAppearance(desktop, [member({ mark_color: "#2b97ff" })])).toEqual({ color: "#ff9234", emoji: "🐙", hidden: false });
    expect(accountAppearance({}, [member({ marks_supported: true, mark_color: "red", mark_emoji: "A" })])).toEqual({ color: "", emoji: "", hidden: false });
  });
  it("shows the emoji only with emoji account marks", () => {
    const work = account([member()], "a", "Work");
    const values = { [`accounts/mark/${profileToken("", "claude-a1")}`]: "🦊" };
    expect(accountMark(work, values)).toMatchObject({ glyph: "W", isEmoji: false, emoji: "🦊" });
    expect(accountMark(work, { ...values, "sessions/accountMarks": "emoji" })).toMatchObject({ glyph: "🦊", isEmoji: true });
    expect(accountMark(account([member()], "b", "Work"), { "sessions/accountMarks": "emoji" })).toMatchObject({ glyph: "W", isEmoji: false });
  });
  it("encodes the computer and profile without a path separator", () => {
    expect(profileToken("mac/studio", "id")).not.toContain("/");
  });
});

describe("row marks", () => {
  const local = member(), peer = member({ key: `studio${US}claude-a1`, host: "studio", machine: "studio" });
  const work = account([local, peer], "claude-work", "Work");
  const view = { ...emptyAccounts(), accounts: [work] };
  const index = markIndex(view);

  it("resolves a row by its computer and recorded profile", () => {
    expect(rowMark(row(), index, {})).toMatchObject({ key: "claude-work", label: "Work", letter: "W", machine: "workstation" });
    expect(rowMark(row({ computer: "studio" }), index, {})).toMatchObject({ machine: "studio" });
  });
  it("shows no mark without a reported binding or for an unknown profile", () => {
    expect(rowMark(row({ accountReported: false }), index, {})).toBeUndefined();
    expect(rowMark(row({ accountId: "" }), index, {})).toBeUndefined();
    expect(rowMark(row({ accountId: "claude-gone" }), index, {})).toBeUndefined();
    expect(rowMark(row({ computer: "elsewhere" }), index, {})).toBeUndefined();
    expect(rowMark(row({ cmd: "codex" }), index, {})).toBeUndefined();
  });
});

describe("account label visibility (zerus-sa9qn.13)", () => {
  const local = member();
  const work = account([local, member({ key: `studio${US}claude-a1`, host: "studio", machine: "studio" })], "claude-work", "Work");
  const index = markIndex({ ...emptyAccounts(), accounts: [work] });

  it("shows marks by default and hides them all when the option is off", () => {
    expect(rowMark(row(), index, {})).toBeDefined();
    expect(rowMark(row(), index, { "sessions/showAccountMarks": true })).toBeDefined();
    expect(rowMark(row(), index, { "sessions/showAccountMarks": false })).toBeUndefined();
  });

  it("hides one account's mark when any of its profiles carries the choice, keeping its appearance", () => {
    const values = { [`accounts/hide/${profileToken("studio", "claude-a1")}`]: true, [`accounts/color/${profileToken("", "claude-a1")}`]: "#2b97ff" };
    expect(accountAppearance(values, work.members)).toEqual({ color: "#2b97ff", emoji: "", hidden: true });
    expect(rowMark(row(), index, values)).toBeUndefined();
    expect(rowMark(row(), index, { [`accounts/hide/${profileToken("", "claude-a1")}`]: false })).toBeDefined();
  });
});
