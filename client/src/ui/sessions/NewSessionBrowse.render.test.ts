// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createComponent } from "solid-js";
import { render } from "solid-js/web";
import type { createNewSessionStore, NewSessionActions } from "../../store/newSession";
import type { NewSessionFormInput } from "../../types";

type Model = ReturnType<typeof createNewSessionStore>;
const fixture = vi.hoisted(() => ({ model: undefined as Model | undefined }));

// Route the dialog's singleton entry to the actual production store. Its
// state, publication, actions and Browse child are all real; only RPCs use
// the private fixture. No synthetic state echo stands in for publication.
vi.mock("../../store/newSession", async (original) => {
  const actual = await original<typeof import("../../store/newSession")>();
  const actions = new Proxy({} as NewSessionActions, {
    get: (_target, key) => (...args: unknown[]) => {
      const action = fixture.model?.actions[key as keyof NewSessionActions];
      if (typeof action === "function") return Reflect.apply(action, undefined, args);
    },
  });
  return {
    ...actual,
    newSession: () => fixture.model?.newSession(), newSessionActions: actions,
    closeNewSession: () => fixture.model?.closeNewSession(),
    startNewSession: (confirm: Parameters<Model["startNewSession"]>[0]) => fixture.model?.startNewSession(confirm),
  };
});
vi.mock("../../bridge", () => ({ listDirectories: vi.fn(), createDirectory: vi.fn() }));

import * as bridge from "../../bridge";
import { folder, project, setup, settle } from "../../store/newSessionFake";
import { machineMeta } from "../../store/testing";
import { NewSessionDialog, setNewSessionChild } from "./NewSessionDialog";

let dispose: (() => void) | undefined;
beforeEach(() => {
  vi.clearAllMocks();
  vi.spyOn(HTMLDialogElement.prototype, "showModal").mockImplementation(function (this: HTMLDialogElement) { this.open = true; });
  vi.spyOn(HTMLDialogElement.prototype, "close").mockImplementation(function (this: HTMLDialogElement) { this.open = false; });
});
afterEach(() => {
  setNewSessionChild(undefined);
  fixture.model?.closeNewSession();
  dispose?.(); dispose = undefined;
  fixture.model = undefined;
  document.body.replaceChildren();
  vi.restoreAllMocks();
});

describe("remote Windows Browse publication", () => {
  it.each([
    { base: "C:/Work", chosen: "C:/Work/Chosen", displayed: "C:\\Work\\Chosen" },
    { base: "//server/share", chosen: "//server/share/Chosen", displayed: "\\\\server\\share\\Chosen" },
  ])("shows $chosen immediately without changing computers or waiting for form RPCs", async ({ base, chosen, displayed }) => {
    const t = setup((world) => {
      // Path style follows the verified card, regardless of the alias's name.
      world.cards.mac = machineMeta("mac", {}, { platform: "windows" });
      world.projects = [project("work", "Work", [folder("remote", "mac", base)], { is_default: true })];
    });
    fixture.model = t.store;
    await t.open({ project: "work", computer: "mac" });
    vi.mocked(bridge.listDirectories).mockResolvedValue({ path: base, directories: [{ name: "Chosen", path: chosen }] });
    const host = document.body.appendChild(document.createElement("div"));
    dispose = render(() => createComponent(NewSessionDialog, {}), host);
    await settle();
    const computer = document.querySelector<HTMLButtonElement>('[aria-controls="ns-computer-list"]')!;
    const folderChoice = document.querySelector<HTMLButtonElement>('[aria-controls="ns-folder-list"]')!;
    const computerBefore = computer.textContent;
    document.querySelector<HTMLButtonElement>("#ns-browse")!.click();
    await settle();
    expect(bridge.listDirectories).toHaveBeenCalledExactlyOnceWith("mac", base, false);
    const entry = document.querySelector<HTMLElement>('.fb-list [role="option"]')!;
    expect(entry.dataset.path).toBe(chosen);
    entry.click();
    await settle();
    const formCalls = t.world.count("newSessionForm");
    document.querySelector<HTMLElement>(".fb-body")!.closest("dialog")!.querySelector<HTMLButtonElement>(".dlg-foot .primary")!.click();

    // Check in the click's turn, before a queued native form read can answer.
    expect(t.state().host).toBe("mac");
    expect(computer.textContent).toBe(computerBefore);
    expect(t.state().folderPath).toBe(displayed);
    expect(folderChoice.textContent).toBe("Chosen");
    expect(folderChoice.title).toBe(displayed);
    expect(t.world.count("newSessionForm")).toBe(formCalls);
    expect(bridge.createDirectory).not.toHaveBeenCalled();
    expect(t.world.creating()).toEqual([]);
    await settle();
    const nativeForm = t.world.of("newSessionForm").at(-1)?.args[0] as NewSessionFormInput;
    expect(nativeForm.form).toMatchObject({ path: chosen, style: "windows" });
  });
});
