import { describe, expect, it } from "vitest";
import waiting from "../../../core/src/teams/fixtures/cli-waiting.json";
import cards from "../../../core/src/teams/fixtures/cli-cards.json";
import { KNOWN_KINDS, waitingDecisions, waitingKindIcon, waitingKindLabel, waitingKindSentence } from "./decisions";
import { resolveIcon } from "../lib/icons";

// A card's kind in words, one sentence and an icon follow the exact kind the
// store sends. The list is every kind opened on integrate/teams and the
// branches that add kinds (the store: integration, pipeline_acceptance; the
// launch worker: launch_blocked, launch_uncertain, launch_capacity; standing
// confirmations: standing_confirmation; the team actor: team_actor_blocked;
// the timers: report_waiting, silent_lead, late_session; anyone: note). A kind
// not in the list shows a neutral label with its plain kind and a neutral
// sentence and icon, never another kind's text.

const TABLE: Record<string, { label: string; icon: string }> = {
  phase_review: { label: "Phase review", icon: "layers" },
  phase_acceptance: { label: "Phase acceptance", icon: "check-check" },
  note: { label: "Note", icon: "message-square" },
  integration: { label: "Integration", icon: "git-merge" },
  pipeline_acceptance: { label: "Work acceptance", icon: "check" },
  launch_blocked: { label: "Member not started", icon: "circle-alert" },
  launch_uncertain: { label: "Start not confirmed", icon: "circle-question-mark" },
  launch_capacity: { label: "No room for members", icon: "hourglass" },
  standing_confirmation: { label: "Member needs you", icon: "hand" },
  team_actor_blocked: { label: "Team service blocked", icon: "server" },
  report_waiting: { label: "Report waiting", icon: "clock" },
  silent_lead: { label: "Lead not working", icon: "bell" },
  late_session: { label: "Late session", icon: "timer" },
  rejected_merge: { label: "Rejected result is merged", icon: "git-branch" },
};
const PINNED = [...new Set([...waiting.rows, ...cards.rows].map((row) => row.kind))];

describe("a card's kind", () => {
  it("lists every kind of the pinned replies and the branches, each with its own label, sentence and icon", () => {
    expect([...KNOWN_KINDS].sort()).toEqual(Object.keys(TABLE).sort());
    for (const kind of PINNED) expect(KNOWN_KINDS, `pinned kind ${kind}`).toContain(kind);
    const labels = new Set<string>(), sentences = new Set<string>(), icons = new Set<string>();
    for (const kind of KNOWN_KINDS) {
      expect(waitingKindLabel({ kind }), kind).toBe(TABLE[kind].label);
      expect(waitingKindIcon(kind), kind).toBe(TABLE[kind].icon);
      const sentence = waitingKindSentence({ kind, text: "The stored sentence.", parameters: [] });
      expect(sentence, kind).toMatch(/^[A-Z].*\.$/u);
      expect(sentence.split(". ").length, `${kind}: one sentence`).toBe(1);
      labels.add(waitingKindLabel({ kind })); sentences.add(sentence); icons.add(waitingKindIcon(kind));
    }
    expect(labels.size, "no two kinds share a label").toBe(KNOWN_KINDS.length);
    expect(sentences.size, "no two kinds share a sentence").toBe(KNOWN_KINDS.length);
    expect(icons.size, "no two kinds share an icon").toBe(KNOWN_KINDS.length);
  });

  it("draws every kind's icon with its own glyph, the unknown kind's included", () => {
    // Icon names resolve through the client's aliases (circle-alert and
    // triangle-alert are one glyph), and a name without a glyph draws nothing.
    const glyphs = new Map<string, string>();
    for (const kind of [...KNOWN_KINDS, "unknown_kind"]) {
      const glyph = resolveIcon(waitingKindIcon(kind)).html;
      expect(glyph, `${kind}: the icon draws something`).not.toBe("");
      expect(glyphs.get(glyph), `${kind} draws the same glyph as ${glyphs.get(glyph)}`).toBeUndefined();
      glyphs.set(glyph, kind);
    }
  });

  it("calls the pipeline's acceptance by its own name, not a phase's", () => {
    expect(waitingKindLabel({ kind: "pipeline_acceptance" })).toBe("Work acceptance");
    expect(waitingKindIcon("pipeline_acceptance")).toBe("check");
  });

  it.each(["future_phase_acceptance", "unknown_kind", "milestone_review", "owner_word_asked", ""])("an unknown kind %j shows a neutral label with its plain kind", (kind) => {
    const labels = new Set(KNOWN_KINDS.map((known) => waitingKindLabel({ kind: known })));
    const sentences = new Set(KNOWN_KINDS.map((known) => waitingKindSentence({ kind: known, text: "The stored sentence.", parameters: [] })));
    const icons = new Set(KNOWN_KINDS.map((known) => waitingKindIcon(known)));
    expect(waitingKindLabel({ kind })).toBe(`Card: ${kind}`);
    expect(labels.has(waitingKindLabel({ kind }))).toBe(false);
    expect(waitingKindSentence({ kind, text: "The store's own sentence." })).toBe("The store's own sentence.");
    expect(sentences.has(waitingKindSentence({ kind, text: "The store's own sentence." }))).toBe(false);
    expect(waitingKindIcon(kind)).toBe("info");
    expect(icons.has(waitingKindIcon(kind))).toBe(false);
    expect(waitingDecisions({ kind }).map((choice) => choice.decision)).toEqual(["acknowledged"]);
  });
});
