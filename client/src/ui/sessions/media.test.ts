import { describe, expect, it } from "vitest";
import { Lexer } from "marked";
import { parseMarkdown } from "./markdown";
import { mediaClock, mediaKind, mediaName, mediaNote, mediaReferences, referencePath, toolMedia } from "./media";
import { buildTranscript } from "./transcript";
import type { ActivityEvent } from "../../types";
import type { ActivityItem } from "./types";

// Task 248: the media an agent names, found as Qt finds them
// (ActivityView.cpp markdown's media hook and toolMedia; MediaViewer.h MediaFiles::kind).
describe("agent-named media (task 248)", () => {
  it("reads a reference's path and kind as Qt's MediaFiles", () => {
    expect(referencePath("file:///work/shots/home.png")).toBe("/work/shots/home.png");
    expect(referencePath("file:///C:/work/shot.png")).toBe("C:/work/shot.png");
    expect(referencePath("src/ui/shot.png:12:3")).toBe("src/ui/shot.png");
    expect(referencePath("docs/a%20b.png#L3")).toBe("docs/a b.png");
    expect([mediaKind("a.PNG"), mediaKind("C:\\x\\b.jpeg"), mediaKind("spin.gif"), mediaKind("demo.webm"), mediaKind("notes.md"), mediaKind(".png"), mediaKind("dir.d/file")])
      .toEqual(["image", "image", "animation", "video", "none", "none", "none"]);
    expect(mediaName("/work/shots/home.png:4")).toBe("home.png");
  });

  it("collects every file link of a reply that names a picture, animation or video, once each", () => {
    const tree = parseMarkdown([
      "Before and after: [home](/work/shots/home.png) and ![after](shots/after.webp).",
      "",
      "- the [spinner](assets/spin.gif)",
      "- a [recording](demo/run.mp4), [the log](build.log) and [the site](https://example.com/a.png)",
      "",
      "> again [home](/work/shots/home.png)",
      "",
      "| shot |", "| --- |", "| [table](t/cell.jpg) |",
    ].join("\n"), Lexer);
    expect(mediaReferences(tree)).toEqual(["/work/shots/home.png", "shots/after.webp", "assets/spin.gif", "demo/run.mp4", "t/cell.jpg"]);
  });

  it("finds at most four pictures a tool group worked with, never a video (Qt toolMedia)", () => {
    expect(toolMedia([
      "<path>/work/out/chart.png</path>\n<type>image</type>",
      "Wrote C:\\work\\out\\diff.jpg and /work/out/chart.png",
      "saved /tmp/clip.mp4 and /tmp/a.gif /tmp/b.webp /tmp/c.png",
    ])).toEqual(["/work/out/chart.png", "C:\\work\\out\\diff.jpg", "/tmp/a.gif", "/tmp/b.webp"]);
    expect(toolMedia(["no pictures here", "a.png alone"])).toEqual([]);
    // Owner, 2026-11-15: the tail of a relative path or a URL is no file here.
    expect(toolMedia(["relative/a.png"])).toEqual([]);
  });

  it("takes no path from URLs, relative links, web tools or plain listings", () => {
    expect(toolMedia([
      "![Logo](../_static/logo.png)",
      "![](https://mobcitygame.com/wp-content/uploads/2026/03/image-7.png)",
      "see https://host/d.jpg and //host/e.png",
    ])).toEqual([]);
    expect(toolMedia([{ tool: "web_fetch", detail: "Saved /work/shot.png" }])).toEqual([]);
    expect(toolMedia([{ tool: "pwsh", detail: "Mode  Name\n-a--- C:\\x\\assets\\icon.png" }])).toEqual([]);
    expect(toolMedia([{ tool: "pwsh", detail: "Saved C:\\x\\shot.png" }])).toEqual(["C:\\x\\shot.png"]);
    expect(toolMedia([{ tool: "read", detail: "<path>D:\\x\\shot.png</path>\n<type>image</type>" }])).toEqual(["D:\\x\\shot.png"]);
  });

  it("puts a tool group's pictures on its item", () => {
    const events = [
      { seq: 1, type: "PreToolUse", tool: "Bash", detail: "make shots", at: 1 },
      { seq: 2, type: "PostToolUse", tool: "Bash", detail: "Saved /work/shots/home.png", at: 2 },
    ] as ActivityEvent[];
    expect((buildTranscript(events)[0] as ActivityItem).media).toEqual(["/work/shots/home.png"]);
    expect((buildTranscript([events[0]])[0] as ActivityItem).media).toBeUndefined();
  });

  it("words a tile's state as Qt's notes", () => {
    expect(mediaNote(undefined, "arch")).toBe("Loading…");
    expect(mediaNote({ state: "ready", code: "", detail: "" }, "arch")).toBe("");
    expect(mediaNote({ state: "click", code: "", detail: "" }, "arch")).toBe("On arch, click to play");
    expect(["unknown_folder", "missing", "too_large", "offline", "not_media"].map((code) => mediaNote({ state: "error", code, detail: "" }, "arch")))
      .toEqual(["The session folder is unknown", "Not found on this computer", "Too large for a preview", "arch is offline", "This picture cannot be shown."]);
    expect(mediaNote({ state: "error", code: "failed", detail: "hgs: no such file" }, "arch")).toBe("hgs: no such file");
    expect([mediaClock(0), mediaClock(65_400), mediaClock(-5)]).toEqual(["0:00", "1:05", "0:00"]);
  });
});
