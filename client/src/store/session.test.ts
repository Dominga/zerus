import { describe, expect, it, vi } from "vitest";
import { createComputed, createRoot } from "solid-js";
import { receive } from "./session";
import { composer } from "./composer";
import { questions } from "./questions";
import { notices } from "./actions";
import { detailTab, setDetailTab } from "./ui";
import { catalog } from "./catalog";
import { grouping, groupOf } from "./organization";
import { rowOut } from "./testing";
import { emptyBatch, type GroupingOut } from "../types";

// The wiring reads the page's query (`?select=`, store/ui.ts); the tests run without a page.
vi.hoisted(() => { vi.stubGlobal("location", new URL("http://localhost/")); });

const loaded = (): GroupingOut => ({
  loaded: true, default_project: "ungrouped", ready: true, reason: null, busy: false, status: "", status_error: false, conflicts: 0,
  headers: [
    { id: "alpha", name: "Alpha", color: null, accessible: true, collapsed: false, is_default: false, palette: 0 },
    { id: "ungrouped", name: "General", color: null, accessible: true, collapsed: false, is_default: true, palette: 1 },
  ],
});

describe("receive", () => {
  it("applies a batch's rows and its grouping in one reactive update", () => createRoot((dispose) => {
    const seen: string[] = [];
    createComputed(() => { seen.push(`${catalog.order.length} ${grouping().loaded} ${groupOf("\nrun:1")}`); });
    seen.length = 0;
    receive({ ...emptyBatch(), upserts: [rowOut("", 1)], groups: [{ computer: "", key: "run:1", group: "alpha", rank: 1 }], grouping: loaded() });
    // One run with everything applied: never the new row in the default project, nor the headers without the row.
    expect(seen).toEqual(["1 true alpha"]);
    dispose();
  }));
});

describe("composer events and the question card", () => {
  const view = (key: string, question: unknown = null) => ({
    handle: 1, key, loaded: true, rev: 0, text_epoch: 1, text: "", position: 0, anchor: 0, question, input_visible: question === null,
    attachments: [], removed: null, attaching: false, sending: false, waiting: null, uncertain: false, status: "", status_error: false,
    can_send: false, send_label: "Send", available: true, unavailable_reason: "", block_reason: null, attachments_available: true,
    attachments_reason: "", session_status: "", session_status_error: false, session_blocks_input: false, inspect_error: false,
    cache: { visible: false, tone: "quiet", label: "", short_label: "" }, cache_warning: "", cache_details: "",
    context: { visible: false, text: "", tooltip: "", warning: false, tone: "quiet", percent: null, accessible_name: "" }, local: [],
    cancel_send: false, compacting: false,
    clear: { enabled: false, dismiss: false, title: "", text: "", confirm: "", run: "", conversation: "", cache_enabled: false, running: false, reason: "", cache_reason: "" },
  }) as never;

  it("a notice is Qt's showNotice: an error stays unless it is transient", () => {
    receive({ ...emptyBatch(), composer_events: [
      { kind: "notice", text: "Codex is installing its update in Terminal.", error: false, transient: false },
      { kind: "notice", text: "Answer delivery timed out.", error: true, transient: false },
      { kind: "notice", text: "The native queue is no longer visible.", error: true, transient: true },
    ] });
    const shown = notices().slice(-3).map((n) => [n.tone, n.text, n.sticky]);
    expect(shown).toEqual([
      ["info", "Codex is installing its update in Terminal.", false],
      ["error", "Answer delivery timed out.", true],
      ["error", "The native queue is no longer visible.", false],
    ]);
  });

  it("an open-Terminal request shows the Terminal tab of its own composer only", () => {
    setDetailTab("activity");
    composer.apply(view("studio\nclaude/sample-project/review"));
    receive({ ...emptyBatch(), composer_events: [{ kind: "open_terminal", key: "studio\nclaude/sample-project/other" }] });
    expect(detailTab()).toBe("activity");
    receive({ ...emptyBatch(), composer_events: [{ kind: "open_terminal", key: "studio\nclaude/sample-project/review" }] });
    expect(detailTab()).toBe("terminal");
    setDetailTab("activity");
  });

  it("the card follows the composer's view", () => {
    const card = { card: "question\nk", revision: 1, epoch: 1, asked: "", asked_tip: "", terminal_label: "Open Terminal", view: {} };
    composer.apply(view("studio\nclaude/sample-project/review", card));
    expect(questions.view()?.card).toBe("question\nk");
    composer.apply(view("studio\nclaude/sample-project/review"));
    expect(questions.view()).toBeUndefined();
  });
});
