import { createSignal } from "solid-js";
import { createStore, produce } from "solid-js/store";
import { convertFileSrc } from "@tauri-apps/api/core";
import { inTauri, onSessionMedia, sessionMedia, sessionMediaExternal } from "../bridge";
import { mediaKind, mediaName, mediaNote, type MediaAnswer, type MediaKind } from "../ui/sessions/media";
import { tr } from "../i18n";

// Task 248: the media an agent names (Qt SessionsWindow::loadMedia and its
// MediaViewer). Each tile asks the native side once per session and reference;
// the answer grants a token the `zerus-file` scheme serves. Bounded: the oldest
// tiles' states go first, as a long session names many files.

/** Where a reference resolves: the session's computer, its folder (or the open subagent's) and its machine's name. */
export interface MediaScope { sessionKey: string; computer: string; cwd: string; machine: string }

export interface MediaTileState extends Partial<MediaAnswer> { state: MediaAnswer["state"] }

/** The viewer's file (Qt MediaViewer: "<name> — Zerus"). */
export interface MediaViewerFile { url: string; token: string; kind: MediaKind; name: string; remote: boolean; machine: string }

const LIMIT = 512;
const [tiles, setTiles] = createStore<Record<string, MediaTileState>>({});
const order: string[] = [];
const pending = new Map<number, { key: string; open: boolean; scope: MediaScope }>();
let next = 1;
const [viewer, setViewer] = createSignal<MediaViewerFile>();
export { viewer as mediaViewer };
export const closeMediaViewer = () => setViewer(undefined);

/** What tells the owner a media file could not be opened (Qt showNotice); the Activity wires its toast. */
let notice: (text: string) => void = () => {};
export function onMediaNotice(show: (text: string) => void) { notice = show; }

const keyOf = (scope: MediaScope, href: string) => `${scope.sessionKey}\n${scope.cwd}\n${href}`;
/** The URL the webview reads a granted file from. */
export const mediaUrl = (token: string) => inTauri() ? convertFileSrc(`media/${token}`, "zerus-file") : "";

export const mediaTile = (scope: MediaScope, href: string): MediaTileState | undefined => tiles[keyOf(scope, href)];

function remember(key: string, state: MediaTileState) {
  if (!(key in tiles)) {
    order.push(key);
    if (order.length > LIMIT) { const old = order.shift()!; setTiles(produce((all) => { delete all[old]; })); }
  }
  setTiles(key, state);
}

function apply(answer: MediaAnswer) {
  const asked = pending.get(answer.request);
  if (!asked || answer.state === "loading") return;
  pending.delete(answer.request);
  remember(asked.key, answer);
  if (asked.open) show(answer, asked.scope);
}

function show(answer: MediaAnswer, scope: MediaScope) {
  if (answer.state === "ready" && answer.token) {
    setViewer({ url: mediaUrl(answer.token), token: answer.token, kind: answer.kind, name: answer.name || "", remote: !!scope.computer, machine: scope.machine });
  } else if (answer.state === "error") notice(mediaNote(answer, scope.machine));
}

let listening = false;
function listen() {
  if (listening) return;
  listening = true;
  void onSessionMedia(apply);
}

async function ask(scope: MediaScope, href: string, open: boolean) {
  listen();
  const request = next++;
  const key = keyOf(scope, href);
  pending.set(request, { key, open, scope });
  if (open && scope.computer) notice(tr("Loading %1 from %2…", mediaName(href), scope.machine));
  try {
    const answer = await sessionMedia(request, scope.computer, href, scope.cwd, open);
    // The event may have answered first; a late "loading" never hides its answer.
    if (answer.state === "loading") { if (pending.has(request)) remember(key, { ...answer }); return; }
    if (!pending.has(request)) return;
    pending.delete(request);
    remember(key, answer);
    if (open) show(answer, scope);
  } catch (error) {
    pending.delete(request);
    remember(key, { state: "error", code: "failed", detail: String(error), kind: mediaKind(href) });
    if (open) notice(String(error));
  }
}

/** A tile's first look: asked once per session and reference. */
export function previewMedia(scope: MediaScope, href: string) {
  if (tiles[keyOf(scope, href)]) return;
  remember(keyOf(scope, href), { state: "loading", kind: mediaKind(href) });
  void ask(scope, href, false);
}

/** A click: the viewer, reading the file first when the preview did not (a peer's video, a large file). */
export function openMedia(scope: MediaScope, href: string) {
  const known = tiles[keyOf(scope, href)];
  if (known?.state === "ready" && known.token && known.kind) {
    show(known as MediaAnswer, scope);
    return;
  }
  void ask(scope, href, true);
}

/** Qt's "Open in default app", for the viewer's file. */
export function openMediaExternally(file: MediaViewerFile) {
  void sessionMediaExternal(file.token, file.remote).catch((error) => notice(String(error)));
}
