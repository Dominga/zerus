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

export type Alignment = "left" | "center" | "right";
export type MarkdownBlock =
  | { kind: "p" | "ul" | "code"; text: string }
  | { kind: "table"; header: string[]; align: Alignment[]; rows: string[][] };

/** GFM pipes are escaped even inside code/bold spans. Keep other escapes intact. */
function cells(line: string): { values: string[]; piped: boolean } {
  const text = line.trim(), values: string[] = [];
  let cell = "", piped = false, trailing = false;
  for (let i = 0; i < text.length;) {
    if (text[i] === "\\") {
      let end = i;
      while (text[end] === "\\") end++;
      const escaped = text[end] === "|" && (end - i) % 2 === 1;
      cell += "\\".repeat(end - i - Number(escaped));
      if (escaped) { cell += "|"; end++; }
      i = end; trailing = false;
    } else if (text[i] === "|") {
      values.push(cell.trim()); cell = ""; piped = true; trailing = true; i++;
    } else { cell += text[i++]; trailing = false; }
  }
  values.push(cell.trim());
  if (text.startsWith("|")) values.shift();
  if (trailing) values.pop();
  return { values, piped };
}

const FENCE = /^ {0,3}(`{3,}|~{3,})/;
const BLOCK_START = /^(?: {0,3}(?:>|#{1,6}(?:\s|$)|[-+*]\s+|\d+[.)]\s+)| {0,3}(?:-{3,}|\*{3,}|_{3,})\s*$)/;

function table(header: string, delimiter: string): Extract<MarkdownBlock, { kind: "table" }> | undefined {
  if (/^(?: {4}|\t)/.test(header)) return;
  const head = cells(header), rule = cells(delimiter);
  if ((!head.piped && !rule.piped) || !head.values.length || head.values.length > 64 || head.values.length !== rule.values.length
      || !rule.values.every((cell) => /^:?-+:?$/.test(cell))) return;
  return { kind: "table", header: head.values, rows: [], align: rule.values.map((cell) =>
    cell.endsWith(":") ? cell.startsWith(":") ? "center" : "right" : "left") };
}

/** Structured blocks, with table line boundaries kept before rendering inlines. */
export function markdownBlocks(text: string): MarkdownBlock[] {
  const lines = text.replace(/\r\n?/g, "\n").split("\n"), out: MarkdownBlock[] = [];
  let paragraph: string[] = [];
  const flush = () => {
    if (paragraph.length) {
      const text = paragraph.join("\n");
      out.push({ kind: text.startsWith("- ") ? "ul" : "p", text });
      paragraph = [];
    }
  };
  for (let i = 0; i < lines.length;) {
    const line = lines[i], fence = FENCE.exec(line);
    if (!line.trim()) { flush(); i++; continue; }
    if (fence) {
      flush();
      const marker = fence[1], code: string[] = [];
      i++;
      while (i < lines.length) {
        const close = FENCE.exec(lines[i]);
        if (close && close[1][0] === marker[0] && close[1].length >= marker.length
            && !lines[i].slice(close[0].length).trim()) { i++; break; }
        code.push(lines[i++]);
      }
      out.push({ kind: "code", text: code.join("\n") });
      continue;
    }
    const grid = i + 1 < lines.length ? table(line, lines[i + 1]) : undefined;
    if (grid) {
      flush();
      const start = i;
      let overflow = false;
      i += 2;
      while (i < lines.length && lines[i].trim() && !FENCE.test(lines[i]) && !BLOCK_START.test(lines[i])) {
        // Ragged rows must not amplify a short input into millions of empty
        // cells. Oversized tables keep their complete literal source instead.
        if ((grid.rows.length + 2) * grid.header.length > 4096) overflow = true;
        if (!overflow) {
          const row = cells(lines[i]).values;
          grid.rows.push(grid.header.map((_, index) => row[index] ?? ""));
        }
        i++;
      }
      out.push(overflow ? { kind: "p", text: lines.slice(start, i).join("\n") } : grid);
    } else { paragraph.push(line); i++; }
  }
  flush();
  return out;
}
