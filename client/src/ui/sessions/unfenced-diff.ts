// Agents paste `git diff` output into messages without a code fence
// (zerus-jj9jp). As Markdown, its + and - lines read as bullets, indented
// context as separate code blocks and @@ headers as prose. Before the parse,
// an unambiguous unified diff outside an existing fence becomes one diff code
// block; anything uncertain stays as written.
//
// A diff starts at column 0 with `diff --git` (optional extended headers) or a
// `--- `/`+++ ` pair, and its first hunk must be complete. Each hunk ends
// exactly by its @@ counts; a context line that lost its leading space reads
// as empty context. Further hunks and file sections join the same block, and
// it stops at the first line that does not fit. One pass, linear in the lines.

const HUNK = /^@@ -\d+(?:,(\d+))? \+\d+(?:,(\d+))? @@/;
const GIT = /^diff --git \S+ \S/;
const OLD = /^--- \S/;
const NEW = /^\+\+\+ \S/;
const EXTENDED = /^(?:index [0-9a-f]+\.\.[0-9a-f]+(?: [0-7]+)?|(?:old|new|deleted file|new file) mode [0-7]+|(?:dis)?similarity index \d+%|(?:rename|copy) (?:from|to) \S.*|Binary files \S.* differ)$/;
const MAX_HEADERS = 16;
const OPEN_FENCE = /^ {0,3}(`{3,}(?=[^`]*$)|~{3,})/;
const CLOSE_FENCE = /^ {0,3}(`{3,}|~{3,})[ \t]*$/;
const FENCE_LIKE = /^ {0,3}(`{3,})/;

const line = (lines: string[], at: number) => lines[at].endsWith("\r") ? lines[at].slice(0, -1) : lines[at];

/** Where a diff starting at `start` ends (`end` > `start`), else where to look for the next start. */
function scan(lines: string[], start: number): { end: number; resume: number } {
  const n = lines.length;
  let at = start;
  let end = start;
  while (at < n) {
    let next = at;
    const git = GIT.test(line(lines, next));
    if (git) {
      next++;
      for (let headers = 0; next < n && headers < MAX_HEADERS && EXTENDED.test(line(lines, next)); headers++) next++;
    }
    if (next + 1 >= n || !OLD.test(line(lines, next)) || !NEW.test(line(lines, next + 1))) {
      // A header-only section (a rename, a mode change) joins a block that a hunk opened.
      if (git && end > start && next > at + 1) { at = end = next; continue; }
      break;
    }
    next += 2;
    let hunks = 0;
    for (let header: RegExpExecArray | null; next < n && (header = HUNK.exec(line(lines, next))); hunks++) {
      let old = header[1] === undefined ? 1 : Number(header[1]);
      let added = header[2] === undefined ? 1 : Number(header[2]);
      next++;
      for (; next < n && (old > 0 || added > 0); next++) {
        const text = line(lines, next);
        const kind = text[0];
        if (kind === "\\") continue;
        if (kind === " " || text === "") { if (!old || !added) break; old--; added--; }
        else if (kind === "-") { if (!old) break; old--; }
        else if (kind === "+") { if (!added) break; added--; }
        else break;
      }
      if (old > 0 || added > 0) {
        // The first hunk must be whole; a later one ends at the first line that does not fit.
        return end > start || hunks > 0 ? { end: next, resume: next } : { end: start, resume: Math.max(next, start + 1) };
      }
      while (next < n && line(lines, next).startsWith("\\ ")) next++;
      end = next;
    }
    if (!hunks) break;
    at = next;
  }
  return { end, resume: Math.max(end, start + 1) };
}

/** The message with each unambiguous unfenced unified diff in a diff code block. */
export function fenceUnfencedDiffs(text: string): string {
  if (!text.includes("@@ -")) return text;
  const lines = text.split("\n");
  const out: string[] = [];
  let fence: { mark: string; size: number } | undefined;
  let changed = false;
  // No diff starts inside lines a failed start already read: linear work.
  let from = 0;
  for (let at = 0; at < lines.length;) {
    const current = line(lines, at);
    if (fence) {
      const close = CLOSE_FENCE.exec(current);
      if (close && close[1][0] === fence.mark && close[1].length >= fence.size) fence = undefined;
      out.push(lines[at++]);
      continue;
    }
    const open = OPEN_FENCE.exec(current);
    if (open) {
      fence = { mark: open[1][0], size: open[1].length };
      out.push(lines[at++]);
      continue;
    }
    if (at >= from && (GIT.test(current) || OLD.test(current))) {
      const { end, resume } = scan(lines, at);
      if (end > at) {
        let longest = 0;
        for (let index = at; index < end; index++) longest = Math.max(longest, FENCE_LIKE.exec(line(lines, index))?.[1].length ?? 0);
        const ticks = "`".repeat(Math.max(3, longest + 1));
        // A blank line first ends a paragraph or HTML block above the diff.
        if (out.length && out[out.length - 1].trim()) out.push("");
        // One push per line: WebKit caps a call's arguments near 64K.
        out.push(`${ticks}diff`);
        for (let index = at; index < end; index++) out.push(lines[index]);
        out.push(ticks);
        changed = true;
        at = from = end;
        continue;
      }
      from = resume;
    }
    out.push(lines[at++]);
  }
  return changed ? out.join("\n") : text;
}
