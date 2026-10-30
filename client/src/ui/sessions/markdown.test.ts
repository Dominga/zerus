import { describe, expect, it } from "vitest";
import { inlineParts } from "./markdown";

describe("Activity inline links", () => {
  it("keeps code and bold text while linking local documents and HTTP URLs", () => {
    expect(inlineParts("Use `code`, **bold**, [archive](/tmp/a.tar), and https://example.com/doc.")).toEqual([
      { kind: "text", text: "Use " },
      { kind: "code", text: "code" },
      { kind: "text", text: ", " },
      { kind: "bold", text: "bold" },
      { kind: "text", text: ", " },
      { kind: "file", text: "archive", href: "/tmp/a.tar" },
      { kind: "text", text: ", and " },
      { kind: "external", text: "https://example.com/doc", href: "https://example.com/doc" },
      { kind: "text", text: "." },
    ]);
  });

  it("recognizes file URLs, relative paths, line locations and Windows drives", () => {
    expect(inlineParts("[one](docs/a.md:12) [two](file:///tmp/b.md) [three](C:/work/c.cpp:4)")).toEqual([
      { kind: "file", text: "one", href: "docs/a.md:12" },
      { kind: "text", text: " " },
      { kind: "file", text: "two", href: "file:///tmp/b.md" },
      { kind: "text", text: " " },
      { kind: "file", text: "three", href: "C:/work/c.cpp:4" },
    ]);
    expect(inlineParts("[back](C:\\work\\c.cpp:4)")[0]).toEqual({ kind: "file", text: "back", href: "C:\\work\\c.cpp:4" });
  });

  it("autolinks www addresses and leaves other schemes as text", () => {
    expect(inlineParts("See www.example.com/docs and [custom](mailto:person@example.com)")).toEqual([
      { kind: "text", text: "See " },
      { kind: "external", text: "www.example.com/docs", href: "http://www.example.com/docs" },
      { kind: "text", text: " and [custom](mailto:person@example.com)" },
    ]);
  });
});
