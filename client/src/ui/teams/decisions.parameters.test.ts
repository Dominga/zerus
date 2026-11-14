import { describe, expect, it } from "vitest";
import catalog from "../../i18n.ru.json";
import { translateWithCatalog } from "../../i18n";
import type { WaitingItem } from "../../store/teams";
import { waitingKindSentence } from "./decisions";

// Waiting for you words a card from the service's bounded parameters
// (zerus-63rru.34). The client checks them before any interpolation: a bad
// value or an unknown kind keeps the stored sentence itself, never a generic
// description, and the capacity notice stays team-wide (F5).

const item = (over: Partial<WaitingItem>): WaitingItem => ({
  id: "00000000-0000-4000-8000-00000000beef:1",
  team: "00000000-0000-4000-8000-00000000beef",
  card: 1,
  kind: "launch_blocked",
  node: null,
  text: "Member m1: no_account. The detail the service stored.",
  state: "open",
  by: "local",
  created_ms: 0,
  ...over,
});

describe("a card's parameters", () => {
  it("substitutes a writer's member into the sentence", () => {
    const words = waitingKindSentence(item({ parameters: [["member", "m1"]] }));
    expect(words).toBe(
      "Member m1 could not be started; the card says why and whether Zerus tries again.",
    );
  });

  it("keeps the generic sentence when the writer sent no parameter", () => {
    const words = waitingKindSentence(item({}));
    expect(words).toBe(
      "A team member could not be started; the card says why and whether Zerus tries again.",
    );
    expect(words).not.toContain("MEMBER_MISSING");
  });

  const INVALID: Array<[string, Array<[string, string]>]> = [
    ["an oversized value", [["member", "x".repeat(257)]]],
    ["a control value", [["member", "a\u0000b"]]],
    ["a bidi value", [["member", "a\u202eb"]]],
    ["a line separator", [["member", "a\u2028b"]]],
    ["a duplicate key", [["member", "one"], ["member", "two"]]],
    ["a bad key", [["Member", "one"]]],
    ["too many pairs", Array.from({ length: 9 }, (_, n) => ["k" + "a".repeat(n), "v"])],
    ["an empty value", [["member", "  "]]],
  ];
  it.each(INVALID)("keeps the stored sentence on %s", (_case, parameters) => {
    const card = item({ parameters });
    expect(waitingKindSentence(card)).toBe(card.text);
  });

  it("keeps the stored sentence for a kind this version does not describe", () => {
    const card = item({ kind: "future_card", parameters: [["member", "m1"]] });
    expect(waitingKindSentence(card)).toBe(card.text);
  });

  it("leaves a percent token inside a written value literal", () => {
    const words = waitingKindSentence(item({ parameters: [["member", "m%1"]] }));
    expect(words).toBe(
      "Member m%1 could not be started; the card says why and whether Zerus tries again.",
    );
  });

  it("words the capacity notice team-wide and never calls the budget marker a member", () => {
    const card = item({ kind: "launch_capacity", parameters: [["member", "@budget"]] });
    expect(waitingKindSentence(card)).toBe(
      "New team members wait until there is room for another session or account.",
    );
    expect(waitingKindSentence(card)).not.toContain("@budget");
  });

  it("words a capacity card that names a real member by that member", () => {
    const card = item({ kind: "launch_capacity", parameters: [["member", "m1"]] });
    expect(waitingKindSentence(card)).toBe(
      "Member m1 waits until there is room for another session or account.",
    );
  });
});

describe("the new sentences in Russian", () => {
  const SOURCES = [
    "Member %1 could not be started; the card says why and whether Zerus tries again.",
    "Whether member %1 started is not known; Zerus checks before it starts anything again.",
    "Member %1 needs you: the card says what it asks.",
    "Member %1 waits until there is room for another session or account.",
    "New team members wait until there is room for another session or account.",
  ];

  const russian = catalog as Record<string, string | string[]>;
  it.each(SOURCES)("translates %j, keeping its placeholders", (source) => {
    const translation = typeof russian[source] === "string" ? russian[source] as string : undefined;
    expect(translation, source).toBeTruthy();
    expect(translation, source).not.toBe(source);
    expect(translation, source).toMatch(/^[А-ЯЁ]/u);
    expect(translation, source).not.toContain("\u00b7");
    expect(translation?.match(/%1/g)?.length ?? 0).toBe(source.match(/%1/g)?.length ?? 0);
  });

  it("interpolates the Russian member form without reordering the value", () => {
    const words = translateWithCatalog(
      russian,
      "ru",
      "Member %1 could not be started; the card says why and whether Zerus tries again.",
      "m1",
    );
    expect(words).toContain("m1");
    expect(words).not.toContain("%1");
  });
});
