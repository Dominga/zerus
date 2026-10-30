import { defaultRangeExtractor, type Range } from "@tanstack/solid-virtual";
import { inlineParts, markdownBlocks } from "./markdown";
import type { TranscriptItem } from "./types";

export const activityTexts = new WeakMap<HTMLDivElement, () => string>();
export const activityAll = new WeakMap<HTMLDivElement, boolean>();

export function selectionCoversActivity(scroller: HTMLDivElement): boolean {
  const col = scroller.querySelector(".act-col"), selection = document.getSelection();
  if (!col || !selection || selection.isCollapsed || selection.rangeCount === 0) return false;
  const range = selection.getRangeAt(0);
  return range.startContainer === col && range.startOffset === 0
    && range.endContainer === col && range.endOffset === col.childNodes.length;
}

export function selectActivityAll(scroller: HTMLDivElement) {
  const col = scroller.querySelector(".act-col");
  if (!col) return;
  document.getSelection()?.selectAllChildren(col);
  activityAll.set(scroller, true);
}

export function activitySelectionText(scroller: HTMLDivElement): string {
  const selected = document.getSelection()?.toString() ?? "";
  return activityAll.get(scroller) && selectionCoversActivity(scroller)
    ? activityTexts.get(scroller)?.() ?? selected : selected;
}

const inlineText = (text: string) => inlineParts(text).map((part) => part.text).join("");

function markdownText(text: string): string {
  return markdownBlocks(text).map((block) => {
    if (block.kind === "code") return block.text;
    if (block.kind === "table") return [block.header, ...block.rows]
      .map((row) => row.map(inlineText).join("\t")).join("\n");
    if (block.kind === "ul") return block.text.split("\n").map((line) => inlineText(line.replace(/^- /, ""))).join("\n");
    return inlineText(block.text);
  }).join("\n");
}

const lines = (values: (string | undefined)[]) => values.filter((value): value is string => !!value).join("\n");

export function activityText(items: TranscriptItem[], expanded: Record<string, boolean> = {}): string {
  return items.map((item) => {
    if (item.kind === "media") return lines([
      item.media === "video" ? "You (video)" : "You (voice)", item.at,
      expanded[item.id] ? item.transcript : undefined, item.error,
    ]);
    if (item.kind === "context") return item.body ?? "";
    if (item.kind === "thinking") return lines(["Thinking", item.body]);
    if (item.kind === "report") return lines([
      item.title, item.sender ? `from ${item.sender}` : undefined, item.at,
      expanded[item.id] ? item.body : undefined,
    ]);
    if (item.kind === "tool" || item.kind === "tools") return lines([
      item.title, item.kind === "tools" ? item.body : undefined, item.at,
      item.kind === "tool" ? item.body : undefined,
      expanded[item.id] ? item.lines?.join("\n") : undefined,
    ]);
    const body = item.replies?.length
      ? item.replies.map((reply) => lines(["Question", reply.question, "Your answer", reply.answer])).join("\n")
      : item.kind === "user" ? item.body : item.body ? markdownText(item.body) : undefined;
    return lines([
      item.label ?? (item.kind === "user" ? "You" : item.kind === "agent" ? "Agent" : item.sender ?? "Zerus"),
      item.local?.stamp ?? item.at, item.receipt, body, item.local?.error, item.code,
      item.attachments?.map((attachment, index) => item.local && !item.local.available[index]
        ? `Attachment: ${attachment.name}` : lines([attachment.caption, attachment.meta])).join("\n"),
    ]);
  }).filter(Boolean).join("\n\n");
}

export function activityRange(range: Range, playingIndex: number): number[] {
  const indexes = defaultRangeExtractor(range);
  if (playingIndex < 0 || playingIndex >= range.count || indexes.includes(playingIndex)) return indexes;
  return [...indexes, playingIndex].sort((a, b) => a - b);
}

export function activityVirtualOffset(scroller: HTMLElement, virtual: HTMLElement, start: number): number {
  const edge = scroller.getBoundingClientRect().top + scroller.clientTop;
  return virtual.getBoundingClientRect().top - edge + start;
}
