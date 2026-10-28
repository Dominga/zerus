import { describe, expect, it } from "vitest";
import {
  AVAILABLE, CREATED, EMPTY, FolderBrowser, LISTED, LOADING, TRUNCATED, TYPED, USE_SELECTED, USE_THIS, childPath, validFolderName,
  type FolderBrowserCalls,
} from "./folder-browser";

// The peer folder browser (tray/src/DirectoryDialog.cpp 46-326) over a
// recorded computer: each listing and creation waits until the test answers
// it, so late answers can arrive in any order. Synthetic paths only.

type Json = Record<string, unknown>;
interface Pending { kind: "list" | "create"; path: string; hidden?: boolean; answer(data: Json): void; fail(error: string): void }

function fake() {
  const calls: Pending[] = [];
  const settle = () => new Promise((resolve) => setTimeout(resolve, 0));
  const pending = (kind: Pending["kind"], path: string, hidden?: boolean) => new Promise<Json>((resolve, reject) => {
    calls.push({ kind, path, hidden, answer: resolve, fail: reject });
  });
  const bridge: FolderBrowserCalls = { list: (path, hidden) => pending("list", path, hidden), create: (path) => pending("create", path) };
  const last = (kind: Pending["kind"]) => calls.filter((c) => c.kind === kind).at(-1)!;
  const listing = (path: string, names: string[], extra: Json = {}) => ({ path, parent: path.slice(0, path.lastIndexOf("/")) || "/", directories: names.map((name) => ({ name, path: `${path}/${name}` })), ...extra });
  const answer = async (data: Json, call = last("list")) => { call.answer(data); await settle(); };
  const fail = async (error: string, call: Pending) => { call.fail(error); await settle(); };
  return { calls, bridge, last, listing, answer, fail };
}

describe("the folder browser (DirectoryDialog.cpp)", () => {
  it("opens on its folder, or home, and names its computer", async () => {
    const f = fake();
    const browser = new FolderBrowser(f.bridge, "mac", "");
    expect(f.calls.map((c) => [c.kind, c.path, c.hidden])).toEqual([["list", "~", false]]);
    expect(browser.view()).toMatchObject({ title: "Choose folder — mac", machine: "Folders on mac", path: "~", status: LOADING, chooseEnabled: false, listEnabled: false, newFolderEnabled: false });
    await f.answer(f.listing("/home/example", ["src", "notes"]));
    expect(browser.view()).toMatchObject({ path: "/home/example", current: "/home/example", status: LISTED, chooseEnabled: true, chooseText: USE_THIS, listEnabled: true });
    expect(browser.view().entries.map((e) => e.name)).toEqual(["src", "notes"]);
    expect(browser.chosen()).toBe("/home/example");
    new FolderBrowser(f.bridge, "mac", "/srv/work");
    expect(f.last("list").path).toBe("/srv/work");
  });

  it("navigates with Up, Home, Go, the list and Show hidden folders", async () => {
    const f = fake();
    const browser = new FolderBrowser(f.bridge, "mac", "/srv/work");
    await f.answer(f.listing("/srv/work", ["app"]));
    browser.select("/srv/work/app");
    expect(browser.view().chooseText).toBe(USE_SELECTED);
    expect(browser.chosen()).toBe("/srv/work/app");
    browser.open("/srv/work/app");
    await f.answer(f.listing("/srv/work/app", []));
    expect(browser.view()).toMatchObject({ status: EMPTY, chooseText: USE_THIS, selected: undefined });
    browser.up();
    expect(f.last("list").path).toBe("/srv/work");
    await f.answer(f.listing("/srv/work", ["app"]));
    browser.home();
    expect(f.last("list").path).toBe("~");
    await f.answer({ path: "/home/example", parent: "", directories: [] });
    // The home folder has no parent here: Up goes home again.
    browser.up();
    expect(f.last("list").path).toBe("~");
    await f.answer({ path: "/home/example", parent: "", directories: [] });
    browser.setHidden(true);
    expect([f.last("list").path, f.last("list").hidden]).toEqual(["/home/example", true]);
    await f.answer(f.listing("/home/example", [".config"]));
    browser.editPath("/srv/typed");
    expect(browser.view()).toMatchObject({ status: TYPED, chooseEnabled: false, listEnabled: false, newFolderEnabled: true, dialogEnabled: true });
    browser.go();
    expect(f.last("list").path).toBe("/srv/typed");
  });

  it("shows the status of the last listing in Qt's order", async () => {
    const f = fake();
    const browser = new FolderBrowser(f.bridge, "mac", "/srv");
    await f.answer(f.listing("/srv", ["a"], { truncated: true }));
    expect(browser.view().status).toBe(TRUNCATED);
    browser.go();
    expect(browser.view().status).toBe(LOADING);
    await f.answer(f.listing("/srv", ["a"]));
    expect(browser.view().status).toBe(LISTED);
    browser.go();
    await f.answer(f.listing("/srv", []));
    expect(browser.view().status).toBe(EMPTY);
    browser.go();
    await f.fail("No such folder", f.last("list"));
    expect(browser.view()).toMatchObject({ status: "No such folder", current: "", chooseEnabled: false, listEnabled: true, newFolderEnabled: true });
    expect(browser.view().entries).toEqual([]);
  });

  it("drops a listing that arrives after the browser moved on", async () => {
    const f = fake();
    const browser = new FolderBrowser(f.bridge, "mac", "/srv");
    const first = f.last("list");
    browser.navigate("/work");
    await f.answer(f.listing("/srv", ["old"]), first);
    expect(browser.view()).toMatchObject({ path: "/work", status: LOADING, current: "" });
    await f.answer(f.listing("/work", ["new"]));
    expect(browser.view().entries.map((e) => e.name)).toEqual(["new"]);
    // Typing a path drops the listing on its way, as Qt resets its request.
    browser.go();
    const typed = f.last("list");
    browser.editPath("/elsewhere");
    await f.answer(f.listing("/work", ["late"]), typed);
    expect(browser.view()).toMatchObject({ path: "/elsewhere", status: TYPED });
    expect(browser.view().entries.map((e) => e.name)).toEqual(["new"]);
    // A closed browser takes nothing.
    browser.go();
    browser.dispose();
    await f.answer(f.listing("/elsewhere", ["x"]));
    expect(browser.view().status).toBe(LOADING);
  });

  it("filters the listed folders and drops a hidden selection", async () => {
    const f = fake();
    const browser = new FolderBrowser(f.bridge, "mac", "/srv");
    await f.answer(f.listing("/srv", ["Alpha", "beta", "alphabet"]));
    browser.select("/srv/beta");
    browser.setFilter("ALPHA");
    expect(browser.view().entries.map((e) => e.name)).toEqual(["Alpha", "alphabet"]);
    expect(browser.view()).toMatchObject({ selected: undefined, chooseText: USE_THIS });
    browser.go();
    await f.answer(f.listing("/srv", ["Alpha", "beta"]));
    expect(browser.view().filter).toBe("");
  });

  it("names a new folder only as Qt allows", () => {
    expect(validFolderName("new", "/srv")).toBe(true);
    for (const name of ["", ".", "..", "a/b", "tab\there", "bell\u0007"]) expect(validFolderName(name, "/srv"), JSON.stringify(name)).toBe(false);
    // The whole path, counted in UTF-8 bytes, is at most 4096.
    expect(validFolderName("a".repeat(4096 - 5), "/srv")).toBe(true);
    expect(validFolderName("a".repeat(4096 - 4), "/srv")).toBe(false);
    expect(validFolderName("é".repeat(2047), "/s")).toBe(false);
    expect(validFolderName("é".repeat(2046), "/s")).toBe(true);
    expect(childPath("/", "new")).toBe("/new");
    expect(childPath("/srv", "new")).toBe("/srv/new");
  });

  it("creates a folder inline once, then opens it", async () => {
    const f = fake();
    const browser = new FolderBrowser(f.bridge, "mac", "/srv");
    await f.answer(f.listing("/srv", ["a"]));
    browser.setFilter("a");
    browser.newFolder();
    expect(browser.view()).toMatchObject({ inline: { name: "", createEnabled: false }, filter: "", chooseEnabled: false });
    browser.createInline();
    expect(f.calls.filter((c) => c.kind === "create")).toEqual([]);
    browser.setInlineName("new");
    expect(browser.view().inline?.createEnabled).toBe(true);
    browser.createInline();
    browser.createInline();
    expect(f.calls.filter((c) => c.kind === "create").map((c) => c.path)).toEqual(["/srv/new"]);
    expect(browser.view()).toMatchObject({ status: "Creating folder on mac…", creating: true, controlsEnabled: false, inline: undefined, path: "/srv/new", chooseEnabled: false });
    expect(browser.canClose()).toBe(false);
    // Nothing navigates while the folder is being created.
    browser.go();
    browser.home();
    expect(f.calls.filter((c) => c.kind === "list").length).toBe(1);
    await f.answer({ status: "created", path: "/srv/new", parent: "/srv" }, f.last("create"));
    expect(browser.canClose()).toBe(true);
    expect(f.last("list").path).toBe("/srv/new");
    await f.answer(f.listing("/srv/new", []));
    expect(browser.view()).toMatchObject({ status: CREATED, chooseEnabled: true });
    expect(browser.chosen()).toBe("/srv/new");
  });

  it("reads a failed creation's path again and never sends it again", async () => {
    const f = fake();
    const browser = new FolderBrowser(f.bridge, "mac", "/srv");
    await f.answer(f.listing("/srv", []));
    browser.newFolder();
    browser.setInlineName("new");
    browser.createInline();
    await f.fail("Folder creation could not be confirmed. Open the path to check it before trying again.", f.last("create"));
    expect(f.calls.map((c) => [c.kind, c.path])).toEqual([["list", "/srv"], ["create", "/srv/new"], ["list", "/srv/new"]]);
    expect(browser.view()).toMatchObject({ status: LOADING, current: "", chooseEnabled: false });
    // The folder exists after all: it can be used.
    await f.answer(f.listing("/srv/new", []));
    expect(browser.view()).toMatchObject({ status: AVAILABLE, chooseEnabled: true });
    // Or the read fails too: the creation's own error stays.
    browser.newFolderDialog();
    expect(browser.view().prompt).toMatchObject({ title: "New folder — mac", text: "Create a folder in /srv/new on mac.", name: "" });
    browser.setPromptName("deeper");
    browser.confirmPrompt();
    await f.fail("permission denied", f.last("create"));
    await f.fail("No such folder", f.last("list"));
    expect(browser.view().status).toBe("permission denied");
    expect(f.calls.filter((c) => c.kind === "create").length).toBe(2);
  });

  it("opens the New folder dialog on the typed path's parent", async () => {
    const f = fake();
    const browser = new FolderBrowser(f.bridge, "mac", "/srv");
    await f.answer(f.listing("/srv", []));
    // A typed path that does not exist: its parent and last name.
    browser.editPath("/srv/missing/leaf");
    browser.newFolder();
    expect(browser.view().prompt).toMatchObject({ text: "Create a folder in /srv/missing on mac.", name: "leaf", createEnabled: true });
    browser.cancelPrompt();
    browser.editPath("/top");
    browser.newFolderDialog();
    expect(browser.view().prompt).toMatchObject({ text: "Create a folder in / on mac.", name: "top" });
    browser.cancelPrompt();
    // A relative path names no parent: nothing opens.
    browser.editPath("relative");
    browser.newFolderDialog();
    expect(browser.view().prompt).toBeUndefined();
    browser.editPath("~/projects");
    browser.newFolderDialog();
    expect(browser.view().prompt).toMatchObject({ text: "Create a folder in ~ on mac.", name: "projects" });
    browser.confirmPrompt();
    expect(f.last("create").path).toBe("~/projects");
    expect(browser.view().prompt).toBeUndefined();
  });

  it("carries the inline name into the dialog and cancels the inline row", async () => {
    const f = fake();
    const browser = new FolderBrowser(f.bridge, "mac", "/srv");
    await f.answer(f.listing("/srv", ["a"]));
    browser.newFolder();
    browser.setInlineName("draft");
    browser.newFolderDialog();
    expect(browser.view()).toMatchObject({ inline: undefined, prompt: { name: "draft", text: "Create a folder in /srv on mac." } });
    browser.cancelPrompt();
    browser.newFolder();
    browser.cancelInline();
    expect(browser.view()).toMatchObject({ inline: undefined, chooseEnabled: true, chooseText: USE_THIS });
    expect(f.calls.filter((c) => c.kind === "create")).toEqual([]);
  });
});
