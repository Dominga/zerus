import { beforeEach, expect, it, vi } from "vitest";
import type { TaskEditorView } from "./taskEditor";
import type { FormKind } from "./taskForms";
import type { Session } from "../ui/sessions/types";
const fixture = vi.hoisted(() => ({ view: undefined as TaskEditorView<FormKind> | undefined }));
vi.mock("../bridge", () => ({
  taskFormOpen: vi.fn(async (target) => fixture.view = { ...target, handle: target.mode === "new_card" ? 10 : 20, rev: 0, text: "Imported title", position: 4, anchor: 1, files: [], error: "", blocked: false, saved: false, recovery: false }),
  taskFormEdit: vi.fn(async (_kind, input) => fixture.view = { ...fixture.view!, ...input }),
  taskFormSubmit: vi.fn(async () => fixture.view = { ...fixture.view!, saved: true, text: "", created: "11111111-1111-4111-8111-111111111111" }),
  taskFormCancel: vi.fn(async () => fixture.view = { ...fixture.view!, saved: true, text: "" }),
}));
vi.mock("./organization", () => ({ groupOf: () => "example" }));
vi.mock("./actions", () => ({ notify: vi.fn() }));
vi.mock("./exitFlush", () => ({ exitFlush: { register: vi.fn() } }));
import { taskFormSubmit, taskFormEdit, taskFormCancel } from "../bridge";
import { newCard, taskTitle, createCard } from "./taskForms";
const settle = async () => { for (let i = 0; i < 12; ++i) await Promise.resolve(); };
beforeEach(async () => { newCard.follow(); taskTitle.follow(); await settle(); vi.clearAllMocks(); });
it("loads saved form text and cursor without a task submission", async () => {
  taskTitle.follow({ project: "example", task: "task", mode: "title" }); await settle();
  expect(taskTitle.view()?.text).toBe("Imported title"); expect(taskTitle.view()?.position).toBe(4);
  expect(taskFormSubmit).not.toHaveBeenCalled();
});
it("flushes the title before creating with exact captured session and stage identities", async () => {
  newCard.follow({ project: "example", task: "", mode: "new_card" }); await settle();
  newCard.edit("New card title", 8, 1);
  const row = { key: "peer\nrun:one", catalogKey: "run:one", computer: "peer", name: "codex/example/one", runId: "run", conversationId: "conversation", launchId: "launch", state: "running", tracked: true } as Session;
  let answer!: () => void;
  vi.mocked(taskFormEdit).mockImplementationOnce(async (_kind, input) => { await new Promise<void>((resolve) => { answer = resolve; }); return { ...fixture.view!, ...input }; });
  const operation = createCard("example", true, row); await settle();
  expect(taskFormSubmit).not.toHaveBeenCalled(); row.runId = "other"; row.conversationId = "other"; answer();
  expect(await operation).toBe(true);
  expect(taskFormSubmit).toHaveBeenCalledExactlyOnceWith({ kind: "new_card", handle: 10, rev: 1, recover: false, backlog: true, target: { computer: "peer", name: row.name, run_id: "run", conversation_id: "conversation" }, catalog_key: "run:one", session_id: "launch_id:launch" });
});
it("keeps an uncertain attempt without replay and requests recovery only explicitly", async () => {
  newCard.follow({ project: "example", task: "", mode: "new_card" }); await settle();
  vi.mocked(taskFormSubmit).mockRejectedValueOnce(new Error("unknown outcome"));
  expect(await createCard("example", false)).toBe(false); await settle();
  expect(taskFormSubmit).toHaveBeenCalledTimes(1); expect(newCard.view()?.text).toBe("Imported title");
  newCard.view()!.recovery = true;
  expect(await createCard("example", false)).toBe(true);
  expect(vi.mocked(taskFormSubmit).mock.calls[1][0].recover).toBe(true);
});
it("Escape's explicit title discard settles the saved draft instead of submitting", async () => {
  taskTitle.follow({ project: "example", task: "task", mode: "title" }); await settle();
  taskTitle.edit("Cancelled title", 3, 1); expect(await taskTitle.discard()).toBe(true);
  expect(taskFormCancel).toHaveBeenCalledExactlyOnceWith("title", 20, 1); expect(taskFormSubmit).not.toHaveBeenCalled();
});
