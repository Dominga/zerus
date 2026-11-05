import { describe, expect, it, vi } from "vitest";
import catalog from "../i18n.ru.json";
import type { ComposerView } from "../types";

// The draft store's refused-save error in Russian, through the shipped
// catalog; other core status text keeps its source form.
vi.mock("../i18n", async (original) => {
  const i18n = await original<typeof import("../i18n")>();
  return { ...i18n, tr: (source: string, ...args: (string | number)[]) => i18n.translateWithCatalog(catalog, "ru", source, ...args) };
});
const { statusLine, DRAFTS_NOT_RECORDED } = await import("./composer");

const RUST = import.meta.glob("../../core/src/drafts.rs", { query: "?raw", import: "default", eager: true }) as Record<string, string>;
const view = (status: string) => ({ status, status_error: true, session_status: "", session_status_error: false, unavailable_reason: "", waiting: null }) as unknown as ComposerView;

describe("the draft store's refused-save error", () => {
  it("is the core constant, word for word", () => {
    const drafts = Object.values(RUST)[0];
    expect(drafts).toContain(`pub const NOT_RECORDED: &str = ${JSON.stringify(DRAFTS_NOT_RECORDED)};`);
  });
  it("shows translated in the status line; other core text keeps its source form", () => {
    expect(statusLine(view(DRAFTS_NOT_RECORDED), undefined, false)).toEqual({ text: catalog[DRAFTS_NOT_RECORDED as keyof typeof catalog], error: true });
    expect(statusLine(view(DRAFTS_NOT_RECORDED), undefined, false).text).toContain("Повторить попытку");
    expect(statusLine(view("Could not save the draft: disk full"), undefined, false)).toEqual({ text: "Could not save the draft: disk full", error: true });
  });
});
