import { describe, expect, it } from "vitest";
import { createComputed, createRoot, createSignal } from "solid-js";
import { createSuggestions, suggestedInput } from "./suggestions";

describe("composer suggestions", () => {
  it("publishes from a reactive source without subscribing to its own cache", () => createRoot((dispose) => {
    const s = createSuggestions(), [text, setText] = createSignal("");
    let publications = 0;
    createComputed(() => {
      const value = text();
      if (++publications < 6) s.set("one", value, 10);
    });
    s.set("two", "another session", 11);
    expect(publications).toBe(1);
    setText("next step");
    expect(publications).toBe(2);
    expect(s.get("one")).toBe("next step");
    setText("");
    expect(publications).toBe(3);
    expect(s.get("one")).toBe("");
    s.set("one", "", 0);
    expect(publications).toBe(3);
    dispose();
  }));

  it("keeps one placeholder per session key and hides it while sending waits", () => createRoot((dispose) => {
    const s = createSuggestions();
    s.set("one", "run the focused tests", 10);
    s.set("two", "summarize the diff", 11);
    s.set("one", "run the full suite", 12);
    expect(s.get("one")).toBe("run the full suite");
    expect(s.get("two")).toBe("summarize the diff");
    expect(s.get("one", true)).toBe("");
    dispose();
  }));

  it("never resurrects a dismissed suggestion at the same timestamp", () => createRoot((dispose) => {
    const s = createSuggestions();
    s.set("one", "commit the fix", 10);
    s.dismiss("one");
    s.set("one", "commit the fix", 10);
    expect(s.get("one")).toBe("");
    s.set("one", "commit the fix", 11);
    expect(s.get("one")).toBe("commit the fix");
    dispose();
  }));

  it("clears a suggestion and its dismissal when the source removes it", () => createRoot((dispose) => {
    const s = createSuggestions();
    s.set("one", "old", 10);
    s.dismiss("one");
    s.set("one", "", 0);
    s.set("one", "new", 10);
    expect(s.get("one")).toBe("new");
    dispose();
  }));

  it("accepts Tab only into an unlocked empty field without auto-filling otherwise", () => {
    expect(suggestedInput("", "suggested next step", false)).toBe("suggested next step");
    expect(suggestedInput("existing draft", "suggested next step", false)).toBeUndefined();
    expect(suggestedInput("", "suggested next step", true)).toBeUndefined();
    expect(suggestedInput("", "", false)).toBeUndefined();
  });
});
