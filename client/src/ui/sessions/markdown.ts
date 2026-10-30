export type InlinePart =
  | { kind: "text" | "code" | "bold"; text: string }
  | { kind: "external" | "file"; text: string; href: string };

const TOKEN = /(`[^`]+`|\*\*[^*]+\*\*|\[[^\]]+\]\([^)]+\)|https?:\/\/[^\s<]+|www\.[^\s<]+)/g;
const PUNCTUATION = /[.,;:!?]+$/;

function bare(text: string): { text: string; href: string } {
  const trimmed = text.replace(PUNCTUATION, "");
  return { text: trimmed, href: trimmed.startsWith("www.") ? `http://${trimmed}` : trimmed };
}

export function inlineParts(text: string): InlinePart[] {
  const out: InlinePart[] = [];
  const plain = (value: string) => {
    if (!value) return;
    const last = out.at(-1);
    if (last?.kind === "text") last.text += value;
    else out.push({ kind: "text", text: value });
  };
  let at = 0;
  for (const match of text.matchAll(TOKEN)) {
    const token = match[0];
    const start = match.index ?? 0;
    if (start > at) plain(text.slice(at, start));
    if (token.startsWith("`")) out.push({ kind: "code", text: token.slice(1, -1) });
    else if (token.startsWith("**")) out.push({ kind: "bold", text: token.slice(2, -2) });
    else if (token.startsWith("[")) {
      const link = /^\[([^\]]+)\]\(([^)]+)\)$/.exec(token);
      if (link) {
        const [, label, href] = link;
        if (/^https?:\/\//i.test(href)) out.push({ kind: "external", text: label, href });
        else if (!/^[a-z][a-z0-9+.-]*:/i.test(href) || href.startsWith("file:") || /^[a-z]:[\\/]/i.test(href)) out.push({ kind: "file", text: label, href });
        else plain(token);
      }
    } else {
      const link = bare(token);
      if (link.text) out.push({ kind: "external", text: link.text, href: link.href });
      plain(token.slice(link.text.length));
    }
    at = start + token.length;
  }
  if (at < text.length) plain(text.slice(at));
  return out;
}
