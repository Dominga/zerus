import { describe, expect, it } from "vitest";
import { inlineParts, markdownBlocks } from "./markdown";
import { SCREENSHOT_TABLE } from "../../perf/fixtures/markdownTables";

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

describe("Activity GFM tables", () => {
  it("keeps the reported Russian table's six rows and long last column", () => {
    const blocks = markdownBlocks("Before.\n\n" + SCREENSHOT_TABLE + "\n\nAfter.");
    expect(blocks[0]).toEqual({ kind: "p", text: "Before." });
    const table = blocks[1];
    if (table.kind !== "table") throw Error("The reply did not contain a table");
    expect(table.header).toEqual(["Пункт постановки", "Фактическое состояние"]);
    expect(table.rows).toHaveLength(6);
    expect(table.rows[0][1]).toBe("**Работает в dev**, проверен реальными клиентами");
    expect(table.rows[5][1]).toContain("Произвольное добавление узла с полной проверкой ещё не принято");
    expect(blocks[2]).toEqual({ kind: "p", text: "After." });
  });

  it("supports alignment and optional outer pipes, padding or discarding body cells", () => {
    expect(markdownBlocks("Left | Middle | Right\n:--- | :-: | ---:\none | two\nthree | four | five | ignored")).toEqual([
      { kind: "table", header: ["Left", "Middle", "Right"], align: ["left", "center", "right"],
        rows: [["one", "two", ""], ["three", "four", "five"]] },
    ]);
  });

  it("unescapes cell pipes before guarded inline parsing, including code and bold", () => {
    const [table] = markdownBlocks(["| f\\|oo | Value |", "| --- | --- |", "| b `\\|` az | **\\|** |"].join("\n"));
    if (table.kind !== "table") throw Error("Missing escaped-pipe table");
    expect(table.header[0]).toBe("f|oo");
    expect(inlineParts(table.rows[0][0])).toEqual([
      { kind: "text", text: "b " }, { kind: "code", text: "|" }, { kind: "text", text: " az" },
    ]);
    expect(inlineParts(table.rows[0][1])).toEqual([{ kind: "bold", text: "|" }]);
  });

  it("rejects mismatched headers and delimiters, and ordinary pipe prose", () => {
    expect(markdownBlocks("| one | two |\n| --- |\n| body |")).toEqual([
      { kind: "p", text: "| one | two |\n| --- |\n| body |" },
    ]);
    expect(markdownBlocks("A | B\nwords | more")).toEqual([{ kind: "p", text: "A | B\nwords | more" }]);
    expect(markdownBlocks("Heading\n---")).toEqual([{ kind: "p", text: "Heading\n---" }]);
  });

  it("retains paragraphs, lists and fenced tables as code, including longer and unclosed fences", () => {
    expect(markdownBlocks("Before\n\n- one\n- two\n\n````md\n" + SCREENSHOT_TABLE + "\n```\n````\nAfter")).toEqual([
      { kind: "p", text: "Before" }, { kind: "ul", text: "- one\n- two" },
      { kind: "code", text: SCREENSHOT_TABLE + "\n```" }, { kind: "p", text: "After" },
    ]);
    expect(markdownBlocks("~~~md\n" + SCREENSHOT_TABLE)).toEqual([{ kind: "code", text: SCREENSHOT_TABLE }]);
  });

  it("ends at a blank line or another block, and admits header-only tables", () => {
    expect(markdownBlocks("| A | B |\n| - | - |\nvalue\n\nAfter")).toEqual([
      { kind: "table", header: ["A", "B"], align: ["left", "left"], rows: [["value", ""]] }, { kind: "p", text: "After" },
    ]);
    expect(markdownBlocks("| A |\r\n| --- |\r\n- list")).toEqual([
      { kind: "table", header: ["A"], align: ["left"], rows: [] }, { kind: "ul", text: "- list" },
    ]);
  });

  it("keeps oversized ragged tables literal instead of amplifying empty cells", () => {
    const header = Array(64).fill("column").join(" | "), delimiter = Array(64).fill("---").join(" | ");
    const text = [header, delimiter, ...Array(70).fill("one cell")].join("\n");
    expect(markdownBlocks(text)).toEqual([{ kind: "p", text }]);
  });
});
