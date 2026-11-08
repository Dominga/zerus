import { tr } from "../../i18n";
import type { Inline, MarkdownBlock } from "./markdown";

// Task 248: pictures, animations and videos an agent names, as Qt shows them
// (ActivityView.cpp mediaTiles, toolMedia; MediaViewer.h MediaFiles). The
// native side resolves and reads them (src-tauri session_files/media.rs);
// this module only finds the references and words the tiles.

export type MediaKind = "none" | "image" | "animation" | "video";

/** A tile's size, as Qt's ActivityMedia (320 x 180). */
export const MEDIA_TILE = { width: 320, height: 180 } as const;

/** The path a file reference names: no file: scheme, fragment or :line:column. */
export function referencePath(href: string): string {
  let path = href.trim().replace(/^file:\/\/(?:localhost)?/i, "");
  if (/^\/[a-z]:[\\/]/i.test(path)) path = path.slice(1);
  path = path.replace(/#.*$/, "").replace(/(?::\d+){1,2}$/, "");
  try { path = decodeURI(path); } catch { /* keep the raw path */ }
  return path;
}

/** Qt MediaFiles::kind: by the file's extension. */
export function mediaKind(href: string): MediaKind {
  const name = referencePath(href).split(/[\\/]/).pop() ?? "";
  const dot = name.lastIndexOf(".");
  if (dot <= 0) return "none";
  const suffix = name.slice(dot + 1).toLowerCase();
  if (["png", "jpg", "jpeg", "webp", "bmp"].includes(suffix)) return "image";
  if (suffix === "gif") return "animation";
  if (["mp4", "webm", "mov", "mkv", "m4v", "avi"].includes(suffix)) return "video";
  return "none";
}

/** The file's name, for the tile's strip and the viewer's title. */
export const mediaName = (href: string) => referencePath(href).split(/[\\/]/).pop() || href;

function walkInlines(parts: Inline[], out: string[]) {
  for (const part of parts) {
    if (part.kind === "file") { if (mediaKind(part.href) !== "none" && !out.includes(part.href)) out.push(part.href); }
    if ("children" in part) walkInlines(part.children, out);
  }
}

/** Qt's markdown hook: every file link of a reply that names a picture, animation or video, once each. */
export function mediaReferences(tree: MarkdownBlock[], out: string[] = []): string[] {
  for (const block of tree) {
    switch (block.kind) {
      case "p": case "text": case "heading": walkInlines(block.inline, out); break;
      case "quote": mediaReferences(block.blocks, out); break;
      case "list": for (const item of block.items) mediaReferences(item.blocks, out); break;
      case "table": for (const row of [block.header, ...block.rows]) for (const cell of row) walkInlines(cell, out); break;
      default: break;
    }
  }
  return out;
}

const READ = /<path>([^<\r\n]+)<\/path>\s*<type>image<\/type>/g;
const PATH = /(?:[A-Za-z]:[\\/]|\/)[^\s"'<>|*?]*\.(?:png|jpe?g|gif|webp)/gi;

/** Qt toolMedia: the pictures a tool group worked with, from its finished
 * tools' results (a DeepSeek read, a written file, any absolute path to a
 * picture). Never a video; at most four. */
export function toolMedia(results: readonly string[]): string[] {
  const media: string[] = [];
  const add = (href: string) => {
    const value = href.trim();
    const kind = mediaKind(value);
    if (media.length < 4 && value && kind !== "none" && kind !== "video" && !media.includes(value)) media.push(value);
  };
  for (const detail of results) {
    for (const match of detail.matchAll(READ)) add(match[1]);
    for (const match of detail.matchAll(PATH)) add(match[0]);
  }
  return media;
}

/** What the native side answered (session_files/media.rs MediaAnswer). */
export interface MediaAnswer {
  request: number;
  state: "ready" | "loading" | "click" | "error";
  code: string;
  detail: string;
  token: string;
  kind: MediaKind;
  size: number;
  name: string;
}

/** The note a tile shows instead of the picture, in Qt's words; "" when it shows the file. */
export function mediaNote(answer: Pick<MediaAnswer, "state" | "code" | "detail"> | undefined, machine: string): string {
  if (!answer || answer.state === "loading") return tr("Loading…");
  if (answer.state === "click") return tr("On %1, click to play", machine);
  if (answer.state === "ready") return "";
  switch (answer.code) {
    case "unknown_folder": return tr("The session folder is unknown");
    case "missing": return tr("Not found on this computer");
    case "too_large": return tr("Too large for a preview");
    case "offline": return tr("%1 is offline", machine);
    case "invalid": return tr("The file reference is invalid.");
    case "not_media": return tr("This picture cannot be shown.");
    default: return answer.detail || tr("This picture cannot be shown.");
  }
}

/** "1:05", as Qt's MediaViewer::clock. */
export function mediaClock(milliseconds: number): string {
  const seconds = Math.max(0, Math.floor(milliseconds / 1000));
  return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, "0")}`;
}
