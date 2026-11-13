// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from "vitest";
import { createComponent, createRoot } from "solid-js";
import { render } from "solid-js/web";
import type { CatalogEditorView, CatalogSchema } from "../../types";
import { createCatalogEditor, type CatalogEditorPort } from "../../store/catalogEditor";
import { TeamCatalogEditor } from "./TeamCatalogEditor";

const flush = async () => { for (let i = 0; i < 10; i++) await Promise.resolve(); };
let host: HTMLDivElement | undefined, dispose: (() => void) | undefined, editorDispose: (() => void) | undefined;
afterEach(() => { dispose?.(); editorDispose?.(); host?.remove(); vi.clearAllMocks(); });

function field(label: string): HTMLElement {
  const found = [...host!.querySelectorAll<HTMLElement>(".cat-field")].find((item) => item.querySelector("label")?.textContent === label)!;
  return found;
}

describe("template fields", () => {
  it("shows the saved role and choice values in list rows", async () => {
    const schema: CatalogSchema = {
      roles: [], routing: [], pipelines: [],
      templates: [
        { name: "roster", label: "Roster", hint: "", type: "list", fields: [
          { name: "role", label: "Role", hint: "", type: "text" },
          { name: "count", label: "Members", hint: "", type: "number", min: 0, max: 20 },
          { name: "optional", label: "only if needed", hint: "", type: "bool" },
        ] },
        { name: "tasks", label: "Task skeleton", hint: "", type: "list", fields: [
          { name: "title", label: "Task", hint: "", type: "text" },
          { name: "role", label: "Role", hint: "", type: "text" },
          { name: "deps", label: "After #", hint: "", type: "words" },
          { name: "phase", label: "Phase", hint: "", type: "number", min: 1, max: 20 },
        ] },
        { name: "planning", label: "Planning", hint: "", type: "choice", options: [["lead", "Lead plans"], ["template", "Use template tasks"]] },
      ],
    };
    const view: CatalogEditorView = {
      version: 1, builtin: false,
      roles: [{ id: "coder", name: "Coder" }, { id: "reviewer", name: "Reviewer" }],
      templates: [{ id: "delivery", name: "Delivery", roster: [{ role: "coder", count: 2, optional: true }], tasks: [{ title: "Check", role: "reviewer", deps: [], phase: 1 }], planning: "template" }],
      routing: [], pipelines: [], planning_rules: "", unsaved: 0, edited: {},
    };
    const port = { schema: vi.fn(async () => schema), load: vi.fn(async () => view) } as unknown as CatalogEditorPort;
    const editor = createRoot((dispose) => { editorDispose = dispose; return createCatalogEditor(port); });
    await editor.load();
    host = document.createElement("div"); document.body.append(host);
    dispose = render(() => createComponent(TeamCatalogEditor, { editor }), host);
    await flush();

    [...host.querySelectorAll<HTMLButtonElement>('[role="tab"]')].find((tab) => tab.textContent === "Templates")!.click();
    await flush();

    const rows = host.querySelectorAll<HTMLElement>(".cat-rowedit");
    expect(rows[0].querySelector<HTMLSelectElement>("select")!.value).toBe("coder");
    expect(rows[0].querySelector<HTMLInputElement>('input[type="number"]')!.value).toBe("2");
    expect(rows[0].querySelector<HTMLInputElement>('input[type="checkbox"]')!.checked).toBe(true);
    expect(rows[1].querySelector<HTMLSelectElement>("select")!.value).toBe("reviewer");
    expect(field("Planning").querySelector<HTMLSelectElement>("select")!.value).toBe("template");
  });
});
