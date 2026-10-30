import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { composer } from "../../store/composer";
import { focusMessageEditor } from "../../store/savedDrafts";
import type { ComposerView } from "../../types";

vi.mock("./SessionCard", () => ({
  folderOf: (s: { cwd: string }) => s.cwd.split(/[\\/]/).filter(Boolean).pop() ?? "",
}));

const view = (key: string, loaded: boolean) => ({ handle: 1, key, loaded, text_epoch: 1, text: "", attachments: [], local: [], input_visible: true }) as unknown as ComposerView;

describe("Saved drafts message focus", () => {
  let editor: { focus: () => void };

  beforeEach(() => {
    vi.useFakeTimers();
    composer.apply(undefined);
    const document = { activeElement: undefined as unknown, querySelector: () => editor };
    editor = { focus: vi.fn(() => { document.activeElement = editor; }) };
    vi.stubGlobal("document", document);
  });

  afterEach(() => {
    composer.apply(undefined);
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it("waits for the target composer instead of focusing the session being left", async () => {
    composer.apply(view("\nclaude/p/old", true));
    const focused = focusMessageEditor("\nclaude/p/target");
    await vi.advanceTimersByTimeAsync(25);
    expect(document.activeElement).not.toBe(editor);
    composer.apply(view("\nclaude/p/target", false));
    await vi.advanceTimersByTimeAsync(25);
    expect(document.activeElement).not.toBe(editor);
    composer.apply(view("\nclaude/p/target", true));
    await vi.advanceTimersByTimeAsync(25);
    await focused;
    expect(document.activeElement).toBe(editor);
  });

  it("stops without focusing after the bounded wait", async () => {
    composer.apply(view("\nclaude/p/old", true));
    const focused = focusMessageEditor("\nclaude/p/target");
    await vi.advanceTimersByTimeAsync(2025);
    await focused;
    expect(document.activeElement).not.toBe(editor);
  });
});
