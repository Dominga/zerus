// @vitest-environment happy-dom
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { createComponent } from "solid-js";
import { render } from "solid-js/web";

const opened = vi.hoisted(() => ({ external: [] as string[] }));
vi.mock("./sessionFiles", () => ({
  openExternal: (url: string) => { opened.external.push(url); },
  openSessionFile: () => {},
}));
import { Body } from "./MarkdownBody";
import { loadMarkdown } from "./markdown";

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
