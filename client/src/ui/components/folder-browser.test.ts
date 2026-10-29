import { describe, expect, it } from "vitest";
import {
  AVAILABLE, CREATED, EMPTY, FolderBrowser, LISTED, LOADING, TRUNCATED, TYPED, USE_SELECTED, USE_THIS, browserEnter, childPath, listMenu, promptEnter,
  validFolderName, type BrowserPlace, type FolderBrowserCalls, type PromptPlace,
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

describe("the folder browser's keys and menu (DirectoryDialog.cpp 83-85, 92-99, 162, 303-311)", () => {
  const press = (key: string, extra: Partial<KeyboardEvent> = {}) => ({ key, altKey: false, ctrlKey: false, metaKey: false, shiftKey: false, ...extra });
  const enter = (browser: FolderBrowser, place: BrowserPlace, extra: Partial<KeyboardEvent> = {}) => browserEnter(place, press("Enter", extra), browser.view());
  // What FolderBrowser.tsx does with the prompt's answer.
  const prompt = (browser: FolderBrowser, place: PromptPlace, extra: Partial<KeyboardEvent> = {}) => {
    const action = promptEnter(place, press("Enter", extra));
    if (action === "create") browser.confirmPrompt();
    else if (action === "cancel") browser.cancelPrompt();
    return action;
  };
  const creates = (f: ReturnType<typeof fake>) => f.calls.filter((c) => c.kind === "create").map((c) => c.path);

  it("presses Use this folder from every other control while it is enabled, and nothing while it is not", async () => {
    const f = fake();
    const browser = new FolderBrowser(f.bridge, "mac", "/srv");
    // Loading: Use this folder is off, so Enter on Up, Home, Go, the filter or Show hidden folders does nothing.
    expect(enter(browser, "other")).toBe("ignore");
    await f.answer(f.listing("/srv", ["a", "b"]));
    expect(enter(browser, "other")).toBe("choose");
    browser.select("/srv/b");
    expect(browser.view().chooseText).toBe(USE_SELECTED);
    expect(enter(browser, "other")).toBe("choose");
    expect(browser.chosen()).toBe("/srv/b");
    // Qt's dialog takes Enter only without modifiers; an input method keeps its Enter, and other keys are the controls'.
    for (const extra of [{ shiftKey: true }, { ctrlKey: true }, { altKey: true }, { metaKey: true }]) expect(enter(browser, "other", extra)).toBe("ignore");
    expect(enter(browser, "other", { isComposing: true })).toBe("native");
    expect(enter(browser, "other", { keyCode: 229 })).toBe("native");
    expect(browserEnter("other", press(" "), browser.view())).toBe("native");
    expect(browserEnter("other", press("Escape"), browser.view())).toBe("native");
    // The path field presses Go, the list opens its folder, the context menu triggers its entry.
    expect(enter(browser, "path")).toBe("go");
    expect(enter(browser, "list")).toBe("native");
    expect(enter(browser, "menu")).toBe("native");
    // An edited path turns Use this folder off: Enter in the path still presses Go.
    browser.editPath("/srv/b");
    expect(enter(browser, "other")).toBe("ignore");
    expect(enter(browser, "path")).toBe("go");
    browser.go();
    await f.answer(f.listing("/srv/b", []));
    // The inline row: its name presses Create, its buttons take nothing, and Use this folder is off meanwhile.
    browser.newFolder();
    expect(enter(browser, "inline-name")).toBe("create");
    expect(enter(browser, "inline")).toBe("ignore");
    expect(enter(browser, "other")).toBe("ignore");
    browser.setInlineName("new");
    browser.createInline();
    // While the folder is being created nothing presses anything.
    expect(enter(browser, "other")).toBe("ignore");
    expect(enter(browser, "path")).toBe("ignore");
    expect(creates(f)).toEqual(["/srv/b/new"]);
  });

  it("creates once from Enter on Create folder, and cancels from Enter on Cancel without a call", async () => {
    const f = fake();
    const browser = new FolderBrowser(f.bridge, "mac", "/srv");
    await f.answer(f.listing("/srv", []));
    // Cancel: as its click, the prompt closes and nothing is sent (Qt's default button would create: a listed deviation).
    browser.newFolderDialog();
    browser.setPromptName("kept-out");
    expect(prompt(browser, "cancel")).toBe("cancel");
    expect(browser.view().prompt).toBeUndefined();
    expect(creates(f)).toEqual([]);
    // Modifiers stop the buttons' Enter; an input method keeps it.
    browser.newFolderDialog();
    browser.setPromptName("new");
    expect(prompt(browser, "cancel", { shiftKey: true })).toBe("ignore");
    expect(prompt(browser, "create", { ctrlKey: true })).toBe("ignore");
    expect(prompt(browser, "create", { isComposing: true })).toBe("native");
    expect(prompt(browser, "name", { keyCode: 229 })).toBe("native");
    expect(browser.view().prompt).toMatchObject({ name: "new", createEnabled: true });
    expect(creates(f)).toEqual([]);
    // Create folder, the default button: one creation, however often Enter comes.
    expect(prompt(browser, "create")).toBe("create");
    expect(prompt(browser, "create")).toBe("create");
    expect(creates(f)).toEqual(["/srv/new"]);
    expect(browser.view()).toMatchObject({ prompt: undefined, creating: true });
    // No second prompt opens while it is being created.
    browser.newFolderDialog();
    expect(browser.view().prompt).toBeUndefined();
    await f.answer({ status: "created", path: "/srv/new", parent: "/srv" }, f.last("create"));
    await f.answer(f.listing("/srv/new", []));
    // Enter in the name creates too, but only a valid name.
    browser.newFolderDialog();
    browser.setPromptName("..");
    expect(prompt(browser, "name")).toBe("create");
    expect(browser.view().prompt).toMatchObject({ name: "..", createEnabled: false });
    browser.setPromptName("deeper");
    expect(prompt(browser, "name")).toBe("create");
    expect(creates(f)).toEqual(["/srv/new", "/srv/new/deeper"]);
  });

  it("opens the list's context menu only while the list is enabled", async () => {
    const f = fake();
    const browser = new FolderBrowser(f.bridge, "mac", "/srv");
    // A disabled QListWidget never asks for its menu: while loading, after an edit and while creating.
    expect(listMenu(browser.view())).toBeUndefined();
    await f.answer(f.listing("/srv", ["a"]));
    expect(listMenu(browser.view())).toEqual([
      { label: "New folder", enabled: true, action: "new" }, { label: "New folder dialog…", enabled: true, action: "dialog" },
    ]);
    browser.editPath("/srv/a");
    expect(listMenu(browser.view())).toBeUndefined();
    browser.go();
    await f.fail("No such folder", f.last("list"));
    expect(listMenu(browser.view())?.map((e) => e.enabled)).toEqual([true, true]);
    browser.newFolderDialog();
    browser.setPromptName("new");
    browser.confirmPrompt();
    expect(browser.view().creating).toBe(true);
    expect(listMenu(browser.view())).toBeUndefined();
    // New folder follows its button; New folder dialog… is always on.
    expect(listMenu({ listEnabled: true, newFolderEnabled: false })?.map((e) => e.enabled)).toEqual([false, true]);
  });
});
