// @vitest-environment happy-dom
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { createComponent } from "solid-js";
import { render } from "solid-js/web";

const opened = vi.hoisted(() => ({ external: [] as string[] }));
vi.mock("./sessionFiles", () => ({
  openExternal: (url: string) => { opened.external.push(url); },
  openSessionFile: () => {},
}));
import { Body, HighlightQuery } from "./MarkdownBody";
import { loadMarkdown } from "./markdown";
import { DIFF, MESSAGE } from "./unfenced-diff.fixture";

// zerus-5wmh.13.8.2: the Activity's Markdown renderer, which thinking and
// subagent reports now use too, links inline code that is exactly one web
// address (MarkdownHtml.cpp:299-311,375-408).
let dispose: (() => void) | undefined;
beforeAll(async () => { await loadMarkdown(); });
afterEach(() => { dispose?.(); dispose = undefined; document.body.replaceChildren(); opened.external.length = 0; });

function mount(text: string) {
  const host = document.createElement("div");
  document.body.append(host);
  dispose = render(() => createComponent(Body, { text }), host);
  return host;
}

describe("the Activity Markdown body", () => {
  it("renders an address written as inline code as a link styled as code, opened outside", () => {
    const host = mount("See `https://example.com/docs` and **bold** text.");
    const link = host.querySelector("a")!;
    expect(link.getAttribute("href")).toBe("https://example.com/docs");
    expect(link.querySelector("code")?.textContent).toBe("https://example.com/docs");
    expect(host.querySelector("b")?.textContent).toBe("bold");
    link.click();
    expect(opened.external).toEqual(["https://example.com/docs"]);
  });

  it("keeps other inline code as code, and a code span inside a link as that one link", () => {
    const host = mount("Run `npm test`, then [`https://a.example`](https://b.example).");
    expect([...host.querySelectorAll("code")].map((c) => c.textContent)).toEqual(["npm test", "https://a.example"]);
    const links = host.querySelectorAll("a");
    expect(links).toHaveLength(1);
    expect(links[0].getAttribute("href")).toBe("https://b.example");
  });
});

describe("a search hit's Markdown (zerus-5wmh.13.17.5)", () => {
  it("highlights the query inside formatting, inline code, code blocks and links, and nowhere without a query", () => {
    const text = "Find the **Needle** in `needle()` and [a needle link](https://example.com).\n\n```\nneedle block\n```";
    const host = document.createElement("div");
    document.body.append(host);
    dispose = render(() => createComponent(HighlightQuery.Provider, { value: () => "needle", get children() { return createComponent(Body, { text }); } }), host);
    expect([...host.querySelectorAll("mark")].map((m) => m.textContent)).toEqual(["Needle", "needle", "needle", "needle"]);
    expect(host.querySelector("b mark")?.textContent).toBe("Needle");
    expect(host.querySelector("code mark")?.textContent).toBe("needle");
    expect(host.querySelector("a mark")?.textContent).toBe("needle");
    expect(host.querySelector("pre mark")?.textContent).toBe("needle");
    dispose(); dispose = undefined; host.replaceChildren();
    dispose = render(() => createComponent(Body, { text }), host);
    expect(host.querySelectorAll("mark")).toHaveLength(0);
  });
});

describe("a message with an unfenced unified diff (zerus-jj9jp)", () => {
  it("shows the diff as one code block, its + and - lines never as bullets", () => {
    const host = mount(MESSAGE);
    const code = host.querySelectorAll("pre.act-code");
    expect(code).toHaveLength(1);
    expect(code[0].textContent).toBe(DIFF);
    expect(host.querySelector("p")?.textContent).toBe("Runtime diff:");
    // Only the prose after the diff is a list.
    expect([...host.querySelectorAll("li")].map((li) => li.textContent)).toEqual(["it keeps old totals", "adds one counter"]);
    expect([...host.querySelectorAll("p")].map((p) => p.textContent)).toEqual(["Runtime diff:", "Please review the weighting."]);
  });
});
