import { tr } from "../../i18n";
import { absolute } from "./path-forms";

// Qt parity (I5a): the composer's Reference file (MessageComposer::
// referenceFile). Only the path goes into the draft, as an instruction to the
// agent; the file is never read or uploaded. A path on this computer must be
// an existing file (src-tauri composer/reference.rs checks it); a remote
// session's path is only checked for its form here.

/** The longest path, in UTF-8 bytes. */
export const PATH_BYTES = 32 * 1024;
/** The longest message the reference may make, in UTF-8 bytes (store/composer MAX_TEXT_BYTES). */
export const MESSAGE_BYTES = 64 * 1024;

const utf8 = new TextEncoder();
/** Unicode's control characters (Qt's QChar::Other_Control): line breaks and tabs among them. */
const control = (c: string) => { const n = c.codePointAt(0) ?? 0; return n < 0x20 || (n >= 0x7f && n <= 0x9f); };

/** Why a typed path is refused before the computer is asked, or "": empty, over 32 KiB or with a control character. */
export function pathProblem(value: string): string {
  return !value || utf8.encode(value).length > PATH_BYTES || Array.from(value).some(control)
    ? tr("Enter a file path without line breaks or control characters.") : "";
}

/** A remote session's path: absolute POSIX (not `//`), `~/`, or a Windows drive or share path. */
export function remotePath(value: string): boolean {
  return (value.startsWith("/") && !value.startsWith("//")) || value.startsWith("~/") || absolute(value, "windows");
}

/** The text inserted for `path`: JSON quoting keeps spaces, quotes and Windows separators exact. */
export function referenceText(path: string): string {
  return tr("Use the existing file at %1.", JSON.stringify(path));
}

/**
 * The draft with the reference to `path` in place of the selection
 * `start..end` (either order), a space added on a side that touches other
 * text, and the caret after it; refused when the message would exceed 64 KiB.
 */
export function insertReference(text: string, start: number, end: number, path: string): { text: string; caret: number } | { error: string } {
  const begin = Math.max(0, Math.min(start, end, text.length));
  const finish = Math.max(begin, Math.min(text.length, Math.max(start, end)));
  let reference = referenceText(path);
  if (begin > 0 && !/\s/u.test(text[begin - 1])) reference = ` ${reference}`;
  if (finish < text.length && !/\s/u.test(text[finish])) reference = `${reference} `;
  const next = text.slice(0, begin) + reference + text.slice(finish);
  if (utf8.encode(next).length > MESSAGE_BYTES) return { error: tr("Message exceeds 64 KiB.") };
  return { text: next, caret: begin + reference.length };
}
