import { describe, expect, it } from "vitest";
import { Lexer } from "marked";
import { parseMarkdown, type MarkdownBlock } from "./markdown";
import { fenceUnfencedDiffs } from "./unfenced-diff";
import { DIFF, MESSAGE } from "./unfenced-diff.fixture";

// zerus-jj9jp: a peer message pasted `git diff` output without a code fence;
// Activity's Markdown turned + and - lines into bullets, indented context into
// separate code blocks and @@ headers into prose. Unambiguous unified diffs
// outside existing fences become one diff code block before the parse.
const fenced = (diff: string, ticks = "```") => `${ticks}diff\n${diff}\n${ticks}`;
const parse = (text: string) => parseMarkdown(text, Lexer);
const p = (text: string): MarkdownBlock => ({ kind: "p", inline: [{ kind: "text", text }] });

describe("an unfenced unified diff in a message", () => {
  it("becomes one diff block, with the prose before and after kept as prose", () => {
    expect(fenceUnfencedDiffs(MESSAGE)).toBe(
      `Runtime diff:\n\n${fenced(DIFF)}\n\nPlease review the weighting.\n- it keeps old totals\n+ adds one counter`);
    const [before, code, after, list] = parse(MESSAGE);
    expect(before).toEqual(p("Runtime diff:"));
    expect(code).toEqual({ kind: "code", text: DIFF });
    expect(after).toEqual(p("Please review the weighting."));
    expect(list.kind).toBe("list");
  });

  it("starts at a ---/+++ pair as well, and wraps each separate diff on its own", () => {
    const plain = "--- old.txt\n+++ new.txt\n@@ -1 +1 @@\n-a\n+b";
    expect(fenceUnfencedDiffs(`${plain}\n\nand\n\n${plain}`)).toBe(`${fenced(plain)}\n\nand\n\n${fenced(plain)}`);
  });

  it("leaves a diff that is already fenced, and text after its fence, as it is", () => {
    for (const text of [fenced(DIFF), `~~~\n${DIFF}\n~~~`, `\`\`\`\`\n${DIFF}\n\`\`\`\nstill inside\n\`\`\`\``, `  \`\`\`patch\n${DIFF}\n  \`\`\``]) {
      expect(fenceUnfencedDiffs(text)).toBe(text);
    }
    expect(fenceUnfencedDiffs(`${fenced("x")}\n${DIFF}`)).toBe(`${fenced("x")}\n\n${fenced(DIFF)}`);
  });

  it("never touches lists, prose or headers that only look like a diff", () => {
    for (const text of [
      "Steps:\n- remove the cache\n+ add a counter\n- done",
      "---\n\n+++ not a header",
      "diff --git a/x b/x\nThat is the command I ran.",
      "diff --git a/x b/x\nindex 1234567..89abcde 100644\n--- a/x\n+++ b/x\nno hunk here",
      "--- a/x\n+++ b/x\n@@ not a hunk header @@",
      "@@ -1,2 +1,2 @@\n-a\n+b\n a",
      "A diff --git a/x b/x line inside prose\n--- a/x",
      "    diff --git a/x b/x\n    --- a/x\n    +++ b/x\n    @@ -1 +1 @@\n    -a\n    +b",
      "> --- a/x\n> +++ b/x\n> @@ -1 +1 @@\n> -a\n> +b",
      "--- a/x\n+++ b/x\n@@ -1,2 +1,2\n-a\n+b",
      "--- a/x\nThen +++ b/x\n@@ -1 +1 @@\n-a\n+b",
      "diff --git a/x b/x\nThat is the command.\n@@ -1 +1 @@\n-a\n+b",
      "Totals:\n--- before\n+++ after\n@@ -1 +1 @@ costs\n- 10\nmore prose",
    ]) expect(fenceUnfencedDiffs(text)).toBe(text);
  });

  it("refuses a first hunk that its lines do not complete", () => {
    expect(fenceUnfencedDiffs("--- a/x\n+++ b/x\n@@ -1,3 +1,3 @@\n a\n-b\nSo that is the change.")).toBe(
      "--- a/x\n+++ b/x\n@@ -1,3 +1,3 @@\n a\n-b\nSo that is the change.");
    expect(fenceUnfencedDiffs("--- a/x\n+++ b/x\n@@ -1,2 +1,2 @@\n a")).toBe("--- a/x\n+++ b/x\n@@ -1,2 +1,2 @@\n a");
  });

  it("ends each hunk exactly by its counts, so following list lines stay outside", () => {
    const diff = "--- a/x\n+++ b/x\n@@ -1,2 +1,2 @@\n a\n-b\n+c";
    expect(fenceUnfencedDiffs(`${diff}\n- first point\n+ second point`)).toBe(`${fenced(diff)}\n- first point\n+ second point`);
    // An empty line is context only while the counts still need one.
    const empty = "--- a/x\n+++ b/x\n@@ -1,3 +1,3 @@\n a\n\n-b\n+c";
    expect(fenceUnfencedDiffs(`${empty}\n\nAfter.`)).toBe(`${fenced(empty)}\n\nAfter.`);
  });

  it("reads omitted and zero counts, and keeps the no-newline marker", () => {
    const created = "diff --git a/n b/n\nnew file mode 100644\nindex 0000000..1111111\n--- /dev/null\n+++ b/n\n@@ -0,0 +1,2 @@\n+one\n+two\n\\ No newline at end of file";
    expect(fenceUnfencedDiffs(`${created}\nok`)).toBe(`${fenced(created)}\nok`);
    const single = "--- a/s\n+++ b/s\n@@ -3 +3 @@ fn\n-x\n+y";
    expect(fenceUnfencedDiffs(single)).toBe(fenced(single));
  });

  it("keeps a header-only file section between files, and ends a later short hunk at the first line that does not fit", () => {
    const renamed = "diff --git a/old b/new\nsimilarity index 100%\nrename from old\nrename to new";
    const first = "--- a/x\n+++ b/x\n@@ -1 +1 @@\n-a\n+b";
    const cut = "diff --git a/y b/y\n--- a/y\n+++ b/y\n@@ -9,4 +9,4 @@\n a\n-b\n+c";
    const text = `diff --git a/x b/x\n${first}\n${renamed}\n${cut}\nThe rest was cut.`;
    expect(fenceUnfencedDiffs(text)).toBe(`${fenced(`diff --git a/x b/x\n${first}\n${renamed}\n${cut}`)}\nThe rest was cut.`);
    // A header-only section cannot open a block, nor a bare hunk follow it.
    expect(fenceUnfencedDiffs(`${renamed}\n@@ -1 +1 @@\n-a\n+b`)).toBe(`${renamed}\n@@ -1 +1 @@\n-a\n+b`);
  });

  it("chooses a fence longer than any fence-like line inside the diff", () => {
    const diff = "--- a/README.md\n+++ b/README.md\n@@ -1,3 +1,3 @@\n ```sh\n-make\n+make test\n ```";
    expect(fenceUnfencedDiffs(diff)).toBe(fenced(diff, "````"));
    expect(parse(diff)).toEqual([{ kind: "code", text: diff }]);
  });

  it("reads Windows line ends", () => {
    const diff = "--- a/x\r\n+++ b/x\r\n@@ -1,2 +1,2 @@\r\n\r\n-a\r\n+b";
    expect(parse(`Note:\r\n${diff}\r\nDone.`)).toEqual([p("Note:"), { kind: "code", text: "--- a/x\n+++ b/x\n@@ -1,2 +1,2 @@\n\n-a\n+b" }, p("Done.")]);
  });

  it("does linear work on large messages and adversarial near-misses", () => {
    const hunk = (n: number) => `@@ -${n},3 +${n},3 @@\n context ${n}\n-old ${n}\n+new ${n}\n context`;
    const big = `--- a/big\n+++ b/big\n${Array.from({ length: 4000 }, (_, n) => hunk(n * 10 + 1)).join("\n")}`;
    expect(big.length).toBeGreaterThan(150_000);
    let started = performance.now();
    expect(fenceUnfencedDiffs(`Big:\n${big}\nend`)).toBe(`Big:\n\n${fenced(big)}\nend`);
    expect(performance.now() - started).toBeLessThan(1000);
    // Thousands of starts, each with a long first hunk that never completes.
    const broken = `--- a/x\n+++ b/x\n@@ -1,99999 +1,99999 @@\n${" ctx\n".repeat(20)}`.repeat(1500);
    started = performance.now();
    expect(fenceUnfencedDiffs(broken)).toBe(broken);
    expect(performance.now() - started).toBeLessThan(1000);
  });

  it("copies a diff of very many short lines without spreading them as call arguments", () => {
    // WebKit's engine caps call arguments near 64K; a 140K-line block must not depend on it.
    const many = `--- a/m\n+++ b/m\n@@ -0,0 +1,140000 @@\n${"+\n".repeat(139_999)}+`;
    expect(fenceUnfencedDiffs(many)).toBe(fenced(many));
  });
});
