// Paste in the composer (Qt's MessageComposer insertFromMimeData, 186-196 at
// f5a822649^): copied local files are attached; otherwise an image is pasted;
// otherwise the text. Plain text never reaches the native clipboard.
import { sameDraft, type DraftBinding } from "./file-reference";

/** What a paste event carries, decided while it is still dispatched. */
export type PasteRoute =
  /** No files: the webview pastes the text itself. */
  | { kind: "text" }
  /** Every text/uri-list entry is a local file: attach these paths. */
  | { kind: "paths"; paths: string[] }
  /**
   * Files without their paths: ask the native clipboard (`ask`), then paste
   * the image, then the text. A paste whose own uri-list names a file that is
   * not local never asks: Qt then attaches none of them (zerus-g01ok).
   */
  | { kind: "native"; ask: boolean; image: File | undefined; text: string };

/**
 * The local paths of a text/uri-list (RFC 2483), as Qt's `allLocal` rule: null
 * unless every entry is a `file:` URL on this computer. A Windows path takes
 * its drive or UNC form.
 */
export function localFilePaths(uriList: string, windows: boolean): string[] | null {
  const entries = uriList.split(/\r?\n/).map((line) => line.trim()).filter((line) => line && !line.startsWith("#"));
  if (!entries.length) return null;
  const paths: string[] = [];
  for (const entry of entries) {
    let url: URL;
    try { url = new URL(entry); } catch { return null; }
    if (url.protocol !== "file:" || url.search || url.hash) return null;
    let path: string;
    try { path = decodeURIComponent(url.pathname); } catch { return null; }
    const host = url.hostname === "localhost" ? "" : url.hostname;
    if (windows) {
      if (host) paths.push(`\\\\${host}${path.replaceAll("/", "\\")}`);
      else if (/^\/[A-Za-z]:\//.test(path)) paths.push(path.slice(1).replaceAll("/", "\\"));
      else return null;
    } else {
      if (host || !path.startsWith("/")) return null;
      paths.push(path);
    }
  }
  return paths;
}

/** How a paste goes, from its clipboard data. */
export function pasteRoute(data: DataTransfer, windows: boolean): PasteRoute {
  const uris = data.types.includes("text/uri-list") ? data.getData("text/uri-list") : "";
  const listed = uris ? localFilePaths(uris, windows) : null;
  if (listed) return { kind: "paths", paths: listed };
  // A list of URLs that are not all local files (Qt's allLocal false).
  const refused = uris.split(/\r?\n/).some((line) => line.trim() && !line.trim().startsWith("#"));
  const files = [...data.items].filter((item) => item.kind === "file").map((item) => item.getAsFile()).filter((file): file is File => !!file);
  if (!files.length) files.push(...data.files);
  if (!files.length) return { kind: "text" };
  return { kind: "native", ask: !refused, image: files.find((file) => file.type.startsWith("image/")), text: data.getData("text/plain") };
}

/** The draft a paste was made in: its composer, text epoch, text and selection (zerus-voy7d). */
export interface PasteBinding extends DraftBinding { text: string; start: number; end: number }

/**
 * Whether a paste that waited for the native clipboard may still apply: the
 * same composer and draft, nothing under way, and neither typing nor a
 * selection change since the paste, which applies at that selection.
 */
export function samePaste(view: Parameters<typeof sameDraft>[0], bound: PasteBinding, locked: boolean, text: string, selection: { start: number; end: number }): boolean {
  return sameDraft(view, bound, locked) && text === bound.text && selection.start === bound.start && selection.end === bound.end;
}

/** What a paste of files without their paths reads and does; tests pass their own. */
export interface PasteDeps {
  /** The native clipboard's copied files (clipboard.rs `clipboard_files`). */
  native: () => Promise<string[] | null>;
  /** The draft the paste was made in is still the composer's, unchanged and free. */
  current: () => boolean;
  attach: (paths: string[]) => void;
  image: (file: File) => void;
  text: (text: string) => void;
}

/** Qt's order for files without their paths: the copied files, else the image, else the text; nothing once the draft changed. */
export async function pasteNative(route: Extract<PasteRoute, { kind: "native" }>, deps: PasteDeps): Promise<void> {
  const paths = route.ask ? await deps.native().catch(() => null) : null;
  if (!deps.current()) return;
  if (paths?.length) deps.attach(paths);
  else if (route.image) deps.image(route.image);
  else if (route.text) deps.text(route.text);
}
