import { tr } from "../../i18n";
import { absolute, type Style } from "./path-forms";

// Reference an existing file (Qt's MessageComposer::referenceFile,
// MessageComposer.cpp:865-959; zerus-5wmh.13.5): the path rules and the
// sentence the draft gets. Only a path is added; the file is never read,
// copied or uploaded.

const PATH_BYTES = 32 << 10;
/** Qt's MaxTextBytes: the draft stays within the message limit. */
export const MESSAGE_BYTES = 64 << 10;

const utf8 = new TextEncoder();

/** Qt's QChar::Other_Control: C0, DEL and C1. */
const control = (path: string) => /[\u0000-\u001f\u007f-\u009f]/.test(path);

/** Non-empty, at most 32 KiB of UTF-8 and no line breaks or other controls. */
export function wellFormed(path: string): boolean {
  return path !== "" && !control(path) && utf8.encode(path).length <= PATH_BYTES;
}

/**
 * A remote session's path, checked for its form only: an absolute path on
 * that computer or a `~/` path. A Windows computer takes a drive or UNC path,
 * another a POSIX one (not `//`); before the computer reports its platform
 * either form is accepted, as Qt accepted both.
 */
export function remoteForm(path: string, style: Style | undefined): boolean {
  if (path.startsWith("~/")) return true;
  const posix = path.startsWith("/") && !path.startsWith("//");
  if (style === "posix") return posix;
  if (style === "windows") return absolute(path, "windows");
  return posix || absolute(path, "windows");
}

/** The draft the dialog opened on (Qt's current()): its composer, key and text epoch. */
export interface DraftBinding { handle: number; key: string; epoch: number }

/** Whether a reference may still go into the draft it was opened for: unchanged and free. */
export function sameDraft(view: { handle: number; key: string; text_epoch: number } | undefined, bound: DraftBinding, locked: boolean): boolean {
  return !!view && !locked && view.handle === bound.handle && view.key === bound.key && view.text_epoch === bound.epoch;
}

/** A computer's path style from its reported platform, unknown before it reports one. */
export function styleOfPlatform(platform: string | null | undefined): Style | undefined {
  if (!platform) return undefined;
  return platform === "windows" ? "windows" : "posix";
}

/**
 * Qt's sentence: the path JSON-quoted, which keeps spaces, quotes and Windows
 * separators. Draft text for the agent, never a shell command or an upload.
 */
export function referenceSentence(path: string): string {
  return tr("Use the existing file at %1.", JSON.stringify(path));
}

/**
 * The draft with the sentence in place of the selection, a space added where
 * it would touch other text; undefined past the 64 KiB message limit.
 */
export function withReference(text: string, start: number, end: number, sentence: string): { text: string; cursor: number } | undefined {
  const from = Math.max(0, Math.min(start, end, text.length));
  const to = Math.min(text.length, Math.max(start, end));
  let inserted = sentence;
  if (from > 0 && !/\s/.test(text[from - 1])) inserted = ` ${inserted}`;
  if (to < text.length && !/\s/.test(text[to])) inserted = `${inserted} `;
  const next = text.slice(0, from) + inserted + text.slice(to);
  if (utf8.encode(next).length > MESSAGE_BYTES) return undefined;
  return { text: next, cursor: from + inserted.length };
}
