// @vitest-environment happy-dom
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { createComponent } from "solid-js";
import { render } from "solid-js/web";

vi.mock("../../bridge", async (original) => ({
  ...(await original<typeof import("../../bridge")>()),
  savedDrafts: vi.fn(async () => ({ items: [], next: null })),
  draftConflicts: vi.fn(async () => ({ items: [], next: null })),
  draftCaptureStatus: vi.fn(async () => null),
  onDraftCaptureChanged: vi.fn(async () => () => {}),
}));

import { openSavedDrafts, savedDrafts, SavedDraftsDialog } from "./SavedDrafts";

let dispose: (() => void) | undefined;
const closeEvents: Array<() => void> = [];
const settle = async () => { for (let i = 0; i < 10; i++) await Promise.resolve(); };
beforeEach(() => {
  // Native close events are tasks, not part of close(). Hold them until the
  // assertion (or a subsequent opening), as a busy WebKit event loop can do.
  vi.spyOn(HTMLDialogElement.prototype, "showModal").mockImplementation(function (this: HTMLDialogElement) { this.open = true; });
  vi.spyOn(HTMLDialogElement.prototype, "close").mockImplementation(function (this: HTMLDialogElement) {
    this.open = false;
    closeEvents.push(() => this.dispatchEvent(new Event("close")));
  });
});
afterEach(() => {
  savedDrafts.close(); dispose?.(); dispose = undefined;
  closeEvents.length = 0; document.body.replaceChildren(); vi.restoreAllMocks();
});

function fixture() {
  const host = document.createElement("div");
  const opener = document.createElement("button");
  const second = document.createElement("button");
  document.body.append(opener, second, host);
  dispose = render(() => createComponent(SavedDraftsDialog, {}), host);
  const dialog = host.querySelector<HTMLDialogElement>("dialog.saved-drafts")!;
  const close = dialog.querySelector<HTMLButtonElement>("[data-initial]")!;
  return { opener, second, dialog, close };
}

it("returns focus when closing, before the native close event can run", async () => {
  const d = fixture();
  openSavedDrafts(d.opener, undefined); await settle();
  expect(document.activeElement).toBe(d.close);
  d.close.click();
  expect(d.dialog.open).toBe(false);
  expect(closeEvents).toHaveLength(1);
  expect(document.activeElement).toBe(d.opener);
  closeEvents.shift()!();
  expect(document.activeElement).toBe(d.opener);
});

it("does not let a delayed close event consume a reopened dialog's opener", async () => {
  const d = fixture();
  openSavedDrafts(d.opener, undefined); await settle();
  d.close.click();
  openSavedDrafts(d.second, undefined); await settle();
  closeEvents.shift()!();
  expect(d.dialog.open).toBe(true);
  expect(document.activeElement).toBe(d.close);
  d.close.click();
  expect(document.activeElement).toBe(d.second);
});

it("does not run queued opening focus after an immediate close", async () => {
  const d = fixture();
  openSavedDrafts(d.opener, undefined);
  d.close.click();
  await settle();
  expect(document.activeElement).toBe(d.opener);
});

it("still returns focus for a native close outside the Close action", async () => {
  const d = fixture();
  openSavedDrafts(d.opener, undefined); await settle();
  d.dialog.close();
  closeEvents.shift()!();
  expect(savedDrafts.open()).toBe(false);
  expect(document.activeElement).toBe(d.opener);
});
