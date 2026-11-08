// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from "vitest";
import { createComponent } from "solid-js";
import { render } from "solid-js/web";

const bridge = vi.hoisted(() => ({
  check: vi.fn(async (_dialog: number, _path: string): Promise<unknown> => ({ status: "missing" })),
  pick: vi.fn(async (): Promise<string | null> => null),
  support: vi.fn(async (_dialog: number, _computer: string): Promise<boolean | null> => null),
  checkRemote: vi.fn(async (_dialog: number, _computer: string, _path: string): Promise<unknown> => ({ status: "unchecked" })),
  cancel: vi.fn(async (_dialog: number): Promise<void> => {}),
}));
vi.mock("../../bridge", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../bridge")>()), fileReferenceCheck: bridge.check, fileReferencePick: bridge.pick,
  fileReferenceRemoteSupport: bridge.support, fileReferenceCheckRemote: bridge.checkRemote, fileReferenceCancel: bridge.cancel,
}));

import { FileReferenceDialog, type ReferenceTarget } from "./FileReferenceDialog";

let dispose: (() => void) | undefined;
afterEach(() => { dispose?.(); dispose = undefined; document.body.replaceChildren(); vi.clearAllMocks(); });
const settle = async () => { for (let i = 0; i < 10; i++) await Promise.resolve(); };

function open(target: ReferenceTarget, insert = vi.fn((_sentence: string): string | undefined => undefined)) {
  const close = vi.fn(() => { dispose?.(); dispose = undefined; });
  const host = document.createElement("div"); document.body.append(host);
  dispose = render(() => createComponent(FileReferenceDialog, { target, insert, close }), host);
  const field = host.querySelector<HTMLInputElement>("input")!;
  const button = (label: string) => [...host.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent === label);
  const type = (value: string) => { field.value = value; field.dispatchEvent(new Event("input", { bubbles: true })); };
  const error = () => host.querySelector("[role=alert]")?.textContent ?? "";
  return { host, field, button, type, error, insert, close };
}

const local: ReferenceTarget = { local: true, machine: "", host: "", style: undefined };
const remote: ReferenceTarget = { local: false, machine: "Studio", host: "studio", style: "posix" };

describe("Reference an existing file dialog", () => {
  it("says what it does on this computer and checks the file before inserting its path", async () => {
    bridge.check.mockResolvedValueOnce({ status: "file", path: "/home/example/notes/plan.md" });
    const d = open(local);
    expect(d.host.querySelector("h2")?.textContent).toBe("Reference file — this computer");
    expect(d.host.textContent).toContain("Only the path is added; the file is not read or uploaded.");
    d.type("~/notes/plan.md");
    d.button("Insert reference")!.click();
    await settle();
    expect(bridge.check).toHaveBeenCalledWith(expect.any(Number), "~/notes/plan.md");
    expect(d.insert).toHaveBeenCalledExactlyOnceWith("Use the existing file at \"/home/example/notes/plan.md\".");
    expect(d.close).toHaveBeenCalled();
  });

  it("asks to retry while the check cannot answer, and refuses a missing file", async () => {
    bridge.check.mockResolvedValueOnce({ status: "busy" });
    const d = open(local);
    d.type("/mnt/stale/report.pdf");
    d.button("Insert reference")!.click();
    await settle();
    expect(d.error()).toBe("Cannot check this path now.");
    expect(d.button("Retry")).toBeDefined();
    bridge.check.mockResolvedValueOnce({ status: "missing" });
    d.button("Retry")!.click();
    await settle();
    expect(d.error()).toBe("Choose an existing file on this computer.");
    expect(bridge.check).toHaveBeenCalledTimes(2);
    // The same dialog number both times: Rust allows it one outstanding check.
    expect(bridge.check.mock.calls[0][0]).toBe(bridge.check.mock.calls[1][0]);
    expect(d.insert).not.toHaveBeenCalled();
  });

  it("drops a late answer once the dialog closed", async () => {
    let answer!: (value: unknown) => void;
    bridge.check.mockImplementationOnce(() => new Promise((resolve) => { answer = resolve; }));
    const d = open(local);
    d.type("/srv/report.pdf");
    d.button("Insert reference")!.click();
    d.button("Cancel")!.click();
    answer({ status: "file", path: "/srv/report.pdf" });
    await settle();
    expect(d.insert).not.toHaveBeenCalled();
  });

  it("checks an older computer's path for its form only and says its existence is not checked", async () => {
    bridge.support.mockResolvedValueOnce(false);
    const d = open(remote);
    await settle();
    expect(bridge.support).toHaveBeenCalledWith(expect.any(Number), "studio");
    expect(d.host.querySelector("h2")?.textContent).toBe("Reference file — Studio");
    expect(d.host.textContent).toContain("on Studio (studio), not on this computer. Only the path is added; its existence is not checked here.");
    expect(d.button("Browse…")).toBeUndefined();
    d.type("relative/report.pdf");
    d.button("Insert reference")!.click();
    expect(d.error()).toBe("Enter an absolute path or ~/ path on Studio.");
    // A text field drops line breaks itself; other controls reach the rule.
    d.type("/srv/a\u0007b");
    d.button("Insert reference")!.click();
    expect(d.error()).toBe("Enter a file path without line breaks or control characters.");
    expect(bridge.checkRemote).not.toHaveBeenCalled();
    d.type("~/report.pdf");
    d.button("Insert reference")!.click();
    await settle();
    expect(bridge.check).not.toHaveBeenCalled();
    // Rust answers "unchecked" for a computer without path-info: inserted as before.
    expect(bridge.checkRemote).toHaveBeenCalledExactlyOnceWith(expect.any(Number), "studio", "~/report.pdf");
    expect(d.insert).toHaveBeenCalledExactlyOnceWith("Use the existing file at \"~/report.pdf\".");
  });

  // zerus-5wmh.13.18: a computer whose hgs has path-info is asked first.
  it("asks a computer that checks paths, says so, and refuses what is not a file there", async () => {
    bridge.support.mockResolvedValueOnce(true);
    const d = open(remote);
    await settle();
    expect(d.host.textContent).toContain("Enter the path of an existing file on Studio (studio), not on this computer. Only the path is added; the file is not read or uploaded.");
    expect(d.host.textContent).not.toContain("its existence is not checked here");
    bridge.checkRemote.mockResolvedValueOnce({ status: "missing" });
    d.type("~/notes");
    d.button("Insert reference")!.click();
    expect(d.button("Insert reference")!.disabled, "Insert waits for the answer").toBe(true);
    await settle();
    expect(d.error()).toBe("Choose an existing file on Studio.");
    bridge.checkRemote.mockResolvedValueOnce({ status: "unreadable" });
    d.button("Insert reference")!.click();
    await settle();
    expect(d.error()).toBe("This path cannot be inspected on Studio.");
    expect(d.insert).not.toHaveBeenCalled();
    bridge.checkRemote.mockResolvedValueOnce({ status: "file" });
    d.type("~/notes/plan.md");
    d.button("Insert reference")!.click();
    await settle();
    // The path is inserted as typed: the computer's hgs expands ~/ itself.
    expect(d.insert).toHaveBeenCalledExactlyOnceWith("Use the existing file at \"~/notes/plan.md\".");
  });

  it("offers Retry while the computer cannot answer, and learns it checks paths from an answer", async () => {
    const d = open(remote);
    await settle();
    expect(d.host.textContent, "unknown until the computer answered").toContain("its existence is not checked here");
    bridge.checkRemote.mockResolvedValueOnce({ status: "busy" });
    d.type("/srv/report.pdf");
    d.button("Insert reference")!.click();
    await settle();
    expect(d.error()).toBe("Cannot check this path now.");
    bridge.checkRemote.mockResolvedValueOnce({ status: "missing" });
    d.button("Retry")!.click();
    await settle();
    expect(d.error()).toBe("Choose an existing file on Studio.");
    expect(d.host.textContent).toContain("Only the path is added; the file is not read or uploaded.");
    expect(bridge.checkRemote.mock.calls[0][0]).toBe(bridge.checkRemote.mock.calls[1][0]);
    expect(d.insert).not.toHaveBeenCalled();
  });

  it("cancels the computer's calls when it closes and drops their late answer", async () => {
    let answer!: (value: unknown) => void;
    bridge.checkRemote.mockImplementationOnce(() => new Promise((resolve) => { answer = resolve; }));
    const d = open(remote);
    d.type("/srv/report.pdf");
    d.button("Insert reference")!.click();
    const id = bridge.checkRemote.mock.calls[0][0];
    expect(bridge.support).toHaveBeenCalledWith(id, "studio");
    d.button("Cancel")!.click();
    expect(bridge.cancel).toHaveBeenCalledExactlyOnceWith(id);
    answer({ status: "file" });
    await settle();
    expect(d.insert).not.toHaveBeenCalled();
  });

  it("never asks about a local path's computer", async () => {
    const d = open(local);
    await settle();
    d.button("Cancel")!.click();
    expect(bridge.support).not.toHaveBeenCalled();
    expect(bridge.cancel).not.toHaveBeenCalled();
  });

  it("keeps the dialog open with the composer's refusal", async () => {
    const d = open(remote, vi.fn(() => "Message exceeds 64 KiB."));
    d.type("/srv/report.pdf");
    d.button("Insert reference")!.click();
    await settle();
    expect(d.error()).toBe("Message exceeds 64 KiB.");
    expect(d.close).not.toHaveBeenCalled();
  });

  it("Browse… fills in the chosen local path", async () => {
    bridge.pick.mockResolvedValueOnce("/home/example/chosen.txt");
    const d = open(local);
    d.button("Browse…")!.click();
    await settle();
    expect(d.field.value).toBe("/home/example/chosen.txt");
  });

  it("drops a pending check's answer once the path is edited, and checks the new one", async () => {
    let answer!: (value: unknown) => void;
    bridge.check.mockImplementationOnce(() => new Promise((resolve) => { answer = resolve; }));
    const d = open(local);
    d.type("/srv/first.txt");
    d.button("Insert reference")!.click();
    expect(d.button("Insert reference")!.disabled).toBe(true);
    d.type("/srv/replacement.txt");
    expect(d.button("Insert reference")!.disabled).toBe(false);
    answer({ status: "file", path: "/srv/first.txt" });
    await settle();
    expect(d.insert, "the checked path changed before its answer returned").not.toHaveBeenCalled();
    expect(d.close).not.toHaveBeenCalled();
    expect(d.error()).toBe("");
    bridge.check.mockResolvedValueOnce({ status: "file", path: "/srv/replacement.txt" });
    d.button("Insert reference")!.click();
    await settle();
    expect(bridge.check).toHaveBeenLastCalledWith(expect.any(Number), "/srv/replacement.txt");
    expect(d.insert).toHaveBeenCalledExactlyOnceWith("Use the existing file at \"/srv/replacement.txt\".");
  });

  it("drops a pending check's answer once Browse… chose another file", async () => {
    let answer!: (value: unknown) => void;
    bridge.check.mockImplementationOnce(() => new Promise((resolve) => { answer = resolve; }));
    bridge.pick.mockResolvedValueOnce("/home/example/chosen.txt");
    const d = open(local);
    d.type("/srv/first.txt");
    d.button("Insert reference")!.click();
    d.button("Browse…")!.click();
    await settle();
    // A typed field takes the chosen path in a fresh node (TextInput's history rule).
    expect(d.host.querySelector<HTMLInputElement>("input")!.value).toBe("/home/example/chosen.txt");
    answer({ status: "file", path: "/srv/first.txt" });
    await settle();
    expect(d.insert).not.toHaveBeenCalled();
    expect(d.close).not.toHaveBeenCalled();
  });
});
