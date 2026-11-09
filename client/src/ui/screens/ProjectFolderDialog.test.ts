// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from "vitest";
import { createComponent } from "solid-js";
import { render } from "solid-js/web";
import type { Folder, PathStyle } from "../../types";
import type { FolderPrompt, Place } from "./project-folders";
import ProjectFolderDialog from "./ProjectFolderDialog";

vi.mock("../../bridge", () => ({}));

const input = (id: string) => document.getElementById(id) as HTMLInputElement;
const submit = () => document.querySelector<HTMLButtonElement>('button[type="submit"]')!;
const settle = async () => { for (let i = 0; i < 4; i++) await Promise.resolve(); };
let dispose: (() => void) | undefined;

function place(style: PathStyle): Place {
  const computer = style === "windows" ? "windows-peer" : "posix-peer";
  return { computer, host: computer, machine_id: `${computer}-node`, label: computer, style };
}

function mount(style: PathStyle = "windows", folders: Folder[] = []) {
  vi.spyOn(HTMLDialogElement.prototype, "showModal").mockImplementation(function (this: HTMLDialogElement) { this.open = true; });
  vi.spyOn(HTMLDialogElement.prototype, "close").mockImplementation(function (this: HTMLDialogElement) { this.open = false; });
  const origin = document.body.appendChild(document.createElement("button"));
  const prompt: FolderPrompt = { project: "work", places: [place(style)], origin };
  const save = vi.fn();
  const host = document.body.appendChild(document.createElement("div"));
  dispose = render(() => createComponent(ProjectFolderDialog, { prompt, busy: false, folders, close: vi.fn(), save }), host);
  return { save };
}

async function setPath(value: string) {
  input("pf-path").value = value;
  input("pf-path").dispatchEvent(new InputEvent("input", { bubbles: true }));
  await settle();
}

const existing = (path: string, style: PathStyle): Folder => ({
  id: "existing", key: "existing", machine: place(style).computer, computer: place(style).label, path, name: "",
});

afterEach(() => {
  dispose?.();
  dispose = undefined;
  document.body.replaceChildren();
  vi.restoreAllMocks();
});

describe("Project folder path validation", () => {
  it.each(["C:\\Work\\Project", "C:/Work/Project", "\\\\server\\share\\Project", "//server/share/Project"])("accepts a remote Windows absolute path with no optional name: %s", async (value) => {
    const { save } = mount();
    await setPath(value);
    expect(input("pf-name").value).toBe("");
    expect(submit().disabled).toBe(false);
    document.querySelector("form")!.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    expect(save).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({ style: "windows" }), value, "");
  });

  it.each(["relative", "C:Work", "\\Work", "/Work", "\\\\server", "\\\\?\\C:\\Work", "C:\\Work\\bad\0"])("rejects an invalid remote Windows path: %s", async (value) => {
    mount();
    await setPath(value);
    expect(submit().disabled).toBe(true);
  });

  it("compares cleaned Windows duplicates ignoring case and separator", async () => {
    mount("windows", [existing("C:/Work/Project", "windows")]);
    await setPath("c:\\work\\project\\.");
    expect(submit().disabled).toBe(true);
    expect(input("pf-path").getAttribute("aria-invalid")).toBe("true");
    await setPath("C:/Work/Project2");
    expect(submit().disabled).toBe(false);
  });

  it("keeps POSIX absolute and case-sensitive duplicate rules", async () => {
    mount("posix", [existing("/Work/Project", "posix")]);
    await setPath("C:/Work/Project");
    expect(submit().disabled).toBe(true);
    await setPath("/Work/Project");
    expect(submit().disabled).toBe(true);
    await setPath("/work/project");
    expect(submit().disabled).toBe(false);
  });
});
