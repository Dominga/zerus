// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from "vitest";
import { createComponent, createSignal } from "solid-js";
import { render } from "solid-js/web";

// The real stores need the desktop bridge; the slots read only whether a
// question or a confirmation is pending, which each test passes in.
vi.mock("../../store/questions", () => ({ questions: { view: () => undefined } }));
vi.mock("../../store/composer", () => ({ composer: { dialog: () => undefined } }));
import { ComposerDialogsSlot, QuestionSlot } from "./lazy-slots";
import { lazyView } from "../lazy-view";

// zerus-ycycu, design-tima2 on #1827: a question or confirmation pending
// before its lazily loaded card arrives is never shown as nothing; a failed
// load shows Retry and the item stays pending.
let dispose: (() => void) | undefined;
afterEach(() => { dispose?.(); dispose = undefined; document.body.replaceChildren(); });
const settle = async () => { for (let i = 0; i < 8; i++) await new Promise((r) => setTimeout(r, 0)); };

/** A view whose module loads when the test says so, or fails. */
function controlled(text: string) {
  let finish: ((fail?: boolean) => void) | undefined;
  let attempts = 0;
  const view = lazyView(() => new Promise<{ default: () => string }>((resolve, reject) => {
    attempts++;
    finish = (fail) => (fail ? reject(new Error("chunk failed")) : resolve({ default: () => text }));
  }));
  return { view, finish: (fail?: boolean) => finish!(fail), attempts: () => attempts };
}

function mount(node: () => ReturnType<typeof createComponent>) {
  const host = document.createElement("div");
  document.body.append(host);
  dispose = render(node, host);
  return host;
}

describe("a lazily loaded question card", () => {
  it("says a question is waiting until the card arrives, and shows nothing when none is pending", async () => {
    const card = controlled("Question card");
    const [pending, setPending] = createSignal(true);
    const host = mount(() => createComponent(QuestionSlot, { view: card.view, pending }));
    expect(host.textContent).toBe("A question is waiting. Loading it…");
    expect(host.querySelector("[role=status]")).not.toBeNull();
    setPending(false);
    expect(host.textContent).toBe("");
    setPending(true);
    card.finish();
    await settle();
    expect(host.textContent).toBe("Question card");
  });

  it("shows Retry when its module fails to load, keeps the question pending, and loads again on Retry", async () => {
    const card = controlled("Question card");
    const pending = vi.fn(() => true);
    const host = mount(() => createComponent(QuestionSlot, { view: card.view, pending }));
    card.finish(true);
    await settle();
    expect(host.querySelector("[role=alert]")?.textContent).toContain("The question could not be shown.");
    expect(card.attempts()).toBe(1);
    host.querySelector<HTMLButtonElement>("[role=alert] button")!.click();
    await settle();
    expect(card.attempts()).toBe(2);
    expect(host.textContent).toBe("A question is waiting. Loading it…");
    card.finish();
    await settle();
    expect(host.textContent).toBe("Question card");
  });
});

describe("the lazily loaded composer confirmations", () => {
  it("says a confirmation is waiting until it arrives, and offers Retry after a failed load", async () => {
    const dialogs = controlled("Confirmation");
    const host = mount(() => createComponent(ComposerDialogsSlot, { view: dialogs.view, pending: () => true }));
    expect(host.textContent).toBe("A confirmation is waiting. Loading it…");
    dialogs.finish(true);
    await settle();
    expect(host.querySelector("[role=alert]")?.textContent).toContain("The confirmation could not be shown.");
    host.querySelector<HTMLButtonElement>("[role=alert] button")!.click();
    await settle();
    dialogs.finish();
    await settle();
    expect(host.textContent).toBe("Confirmation");
  });

  it("shows nothing while loading when no confirmation is pending", () => {
    const dialogs = controlled("Confirmation");
    const host = mount(() => createComponent(ComposerDialogsSlot, { view: dialogs.view, pending: () => false }));
    expect(host.textContent).toBe("");
  });
});
