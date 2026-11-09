import { describe, expect, it, vi } from "vitest";
import { localFilePaths, pasteNative, pasteRoute, type PasteDeps } from "./paste-files";

/** A paste event's clipboard data: typed strings and files. */
function data(strings: Record<string, string>, files: File[] = []): DataTransfer {
  const items = [
    ...Object.keys(strings).map((type) => ({ kind: "string", type, getAsFile: () => null })),
    ...files.map((file) => ({ kind: "file", type: file.type, getAsFile: () => file })),
  ];
  return {
    types: [...Object.keys(strings), ...(files.length ? ["Files"] : [])],
    getData: (type: string) => strings[type] ?? "",
    items, files,
  } as unknown as DataTransfer;
}

describe("copied files in a paste (Qt MessageComposer.cpp:186-196)", () => {
  it("takes a uri-list's local files as paths, decoded", () => {
    expect(localFilePaths("file:///home/example/My%20notes.md\r\nfile://localhost/srv/report.pdf\r\n", false))
      .toEqual(["/home/example/My notes.md", "/srv/report.pdf"]);
    expect(localFilePaths("# copied by a file manager\nfile:///home/example/a.txt", false)).toEqual(["/home/example/a.txt"]);
  });

  it("takes a Windows drive or UNC path in its own form", () => {
    expect(localFilePaths("file:///C:/Users/example/plan%20v2.md", true)).toEqual(["C:\\Users\\example\\plan v2.md"]);
    expect(localFilePaths("file://server/share/report.pdf", true)).toEqual(["\\\\server\\share\\report.pdf"]);
    expect(localFilePaths("file:///home/example/a.txt", true), "no drive on Windows").toBeNull();
  });

  it("refuses the whole list unless every entry is a local file", () => {
    expect(localFilePaths("file:///home/example/a.txt\nhttps://example.com/b.txt", false)).toBeNull();
    expect(localFilePaths("https://example.com/b.txt", false)).toBeNull();
    expect(localFilePaths("file://server/share/a.txt", false), "another computer's file").toBeNull();
    expect(localFilePaths("file:///home/example/%E0%A4%A.txt", false), "a malformed escape").toBeNull();
    expect(localFilePaths("# only a comment\n", false)).toBeNull();
    expect(localFilePaths("", false)).toBeNull();
  });

  it("leaves a paste without files to the webview, a link list included", () => {
    expect(pasteRoute(data({ "text/plain": "hello" }), false)).toEqual({ kind: "text" });
    expect(pasteRoute(data({ "text/uri-list": "https://example.com/", "text/plain": "https://example.com/" }), false)).toEqual({ kind: "text" });
  });

  it("attaches a uri-list's local files before any image or text", () => {
    const image = new File([new Uint8Array([1])], "plan.png", { type: "image/png" });
    expect(pasteRoute(data({ "text/uri-list": "file:///home/example/plan.png", "text/plain": "plan.png" }, [image]), false))
      .toEqual({ kind: "paths", paths: ["/home/example/plan.png"] });
  });

  it("asks the native clipboard for files without their paths, keeping the image and the text", () => {
    const image = new File([new Uint8Array([1])], "image.png", { type: "image/png" });
    const pdf = new File([new Uint8Array([1])], "report.pdf", { type: "application/pdf" });
    expect(pasteRoute(data({ "text/plain": "report.pdf" }, [pdf, image]), false)).toEqual({ kind: "native", ask: true, image, text: "report.pdf" });
    expect(pasteRoute(data({}, [pdf]), false)).toEqual({ kind: "native", ask: true, image: undefined, text: "" });
  });

  it("never asks the native clipboard when the paste's own URL list is not all local (zerus-g01ok)", async () => {
    const image = new File([new Uint8Array([1])], "plan.png", { type: "image/png" });
    const route = pasteRoute(data({ "text/uri-list": "file:///srv/plan.png\nhttps://example.com/remote.png", "text/plain": "plan.png" }, [image]), false);
    expect(route).toEqual({ kind: "native", ask: false, image, text: "plan.png" });
    // The reviewer's control: arboard 3.7 on Linux would return only the local entry.
    const calls: string[] = [];
    const native = vi.fn(async () => ["/srv/plan.png"]);
    if (route.kind === "native") await pasteNative(route, { native, current: () => true, attach: () => { calls.push("attach"); }, image: () => { calls.push("image"); }, text: () => { calls.push("text"); } });
    expect(calls, "Qt attaches none of the files when one URL is not local").toEqual(["image"]);
    expect(native).not.toHaveBeenCalled();
  });
});

describe("a paste of files without their paths", () => {
  const image = new File([new Uint8Array([1])], "image.png", { type: "image/png" });
  function deps(native: string[] | null | Error, current = true) {
    const calls: string[] = [];
    const d: PasteDeps = {
      native: vi.fn(() => native instanceof Error ? Promise.reject(native) : Promise.resolve(native)),
      current: () => current,
      attach: (paths) => calls.push(`attach ${paths.join(",")}`),
      image: (file) => calls.push(`image ${file.name}`),
      text: (text) => calls.push(`text ${text}`),
    };
    return { d, calls };
  }

  it("attaches the copied files, as Qt's allLocal rule", async () => {
    const { d, calls } = deps(["/home/example/plan.png"]);
    await pasteNative({ kind: "native", ask: true, image, text: "plan.png" }, d);
    expect(calls).toEqual(["attach /home/example/plan.png"]);
  });

  it("pastes the image, then the text, when the clipboard has no file list", async () => {
    const one = deps(null);
    await pasteNative({ kind: "native", ask: true, image, text: "image.png" }, one.d);
    expect(one.calls).toEqual(["image image.png"]);
    const two = deps(new Error("no clipboard"));
    await pasteNative({ kind: "native", ask: true, image: undefined, text: "report.pdf" }, two.d);
    expect(two.calls).toEqual(["text report.pdf"]);
    const three = deps([]);
    await pasteNative({ kind: "native", ask: true, image: undefined, text: "" }, three.d);
    expect(three.calls).toEqual([]);
  });

  it("does nothing once the draft it was made in changed", async () => {
    const { d, calls } = deps(["/home/example/plan.png"], false);
    await pasteNative({ kind: "native", ask: true, image, text: "plan.png" }, d);
    expect(calls).toEqual([]);
  });
});
