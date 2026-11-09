/**
 * How much of one event's output an expanded tool row shows (zerus-5wmh.13.8.1):
 * the first part, then a further step per Show more. A multi-megabyte output
 * never becomes one giant block in the Activity list.
 */
export const TOOL_OUTPUT = { first: 2_000, step: 20_000 } as const;

/** The first `shown` characters of `text`, never ending inside a surrogate pair. */
export function shownOutput(text: string, shown: number): string {
  if (text.length <= shown) return text;
  const end = shown > 0 && /[\uD800-\uDBFF]/.test(text[shown - 1]) ? shown - 1 : shown;
  return text.slice(0, end);
}

/** View process for an event's recorded process, when the session's process list holds it. */
export interface ProcessLinks {
  linkable(process: string): boolean;
  open(process: string): void;
}

/** The key of one event's Show more state: its row and place in the row. */
export const outputKey = (row: string, index: number) => `${row}#${index}`;
