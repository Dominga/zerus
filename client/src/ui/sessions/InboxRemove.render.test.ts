// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from "vitest";
import { createComponent, createSignal } from "solid-js";
import { render } from "solid-js/web";
import { InboxRemove, RemoveQueuedDialog, type InboxRemovePorts } from "./InboxRemove";
import type { InboxRemoveReply } from "../../types";
import type { InboxItem } from "./types";

// zerus-5wmh.13.6: Remove asks first (Cancel is the default, Escape is
// Cancel), then sends exactly the message, composer, run and conversation
// the dialog opened for; it never opens while any removal runs.
const item: InboxItem = { state: "not_delivered", messageId: "msg_queued01", run: "run-1", conversation: "conv-1", removable: true };
let dispose: (() => void) | undefined;
afterEach(() => { dispose?.(); document.body.replaceChildren(); });

function setup(reply: InboxRemoveReply = { outcome: "removed" }) {
  const [view, setView] = createSignal<ReturnType<InboxRemovePorts["view"]>>({ handle: 7, inbox_removing: null, inbox_busy: false });
  const remove = vi.fn(async (_h: number, _r: string, _c: string, _m: string) => reply);
  const notify = vi.fn();
  const done = vi.fn();
  const ports: InboxRemovePorts = { view, remove, notify, done };
  const host = document.createElement("div");
  document.body.append(host);
  dispose = render(() => [createComponent(InboxRemove, { item, ports }), createComponent(RemoveQueuedDialog, { ports })], host);
  const button = () => host.querySelector<HTMLButtonElement>(".act-local-actions button")!;
  const dialog = () => host.querySelector<HTMLDialogElement>("dialog")!;
  return { host, setView, remove, notify, done, button, dialog };
}
const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

it("asks first with Cancel focused, and Escape sends nothing", async () => {
  const t = setup();
  expect(t.button().textContent).toBe("Remove");
  t.button().click();
  await settle();
  expect(t.dialog().open).toBe(true);
  expect(t.dialog().textContent).toContain("Remove queued message");
  expect(document.activeElement?.hasAttribute("data-initial")).toBe(true);
  t.dialog().dispatchEvent(new Event("cancel", { cancelable: true }));
  await settle();
  expect(t.dialog().open).toBe(false);
  expect(t.remove).not.toHaveBeenCalled();
  expect(document.activeElement).toBe(t.button());
});

it("Remove sends exactly what the dialog opened for, once, and says how it ended", async () => {
  const t = setup();
  t.button().click();
  await settle();
  // The composer changes while the dialog is open: Remove still names the opened one; Rust checks it.
  t.setView({ handle: 8, inbox_removing: null, inbox_busy: false });
  t.dialog().querySelector<HTMLButtonElement>("[data-confirm]")!.click();
  await settle();
  expect(t.remove).toHaveBeenCalledTimes(1);
  expect(t.remove).toHaveBeenCalledWith(7, "run-1", "conv-1", "msg_queued01");
  expect(t.done).toHaveBeenCalledTimes(1);
  expect(t.notify).toHaveBeenCalledWith("info", "Queued message removed.", false);
  expect(t.dialog().open).toBe(false);
});

it("a changed message is refused with Qt's notice", async () => {
  const t = setup({ outcome: "changed" });
  t.button().click();
  await settle();
  t.dialog().querySelector<HTMLButtonElement>("[data-confirm]")!.click();
  await settle();
  expect(t.notify).toHaveBeenCalledWith("error", "The queued message changed. Check Activity before removing it.", true);
});

it("never opens while a removal runs, and names its own", async () => {
  const t = setup();
  t.setView({ handle: 7, inbox_removing: "msg_queued01", inbox_busy: true });
  expect(t.button().textContent).toBe("Removing…");
  expect(t.button().getAttribute("aria-disabled")).toBe("true");
  t.button().click();
  await settle();
  expect(t.dialog().open).toBe(false);
  t.setView({ handle: 7, inbox_removing: null, inbox_busy: true });
  expect(t.button().textContent).toBe("Remove");
  t.button().click();
  await settle();
  expect(t.dialog().open).toBe(false);
  expect(t.remove).not.toHaveBeenCalled();
});
