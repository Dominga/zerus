import { For, Show, createEffect, createSignal, onCleanup, onMount } from "solid-js";
import { closeMediaViewer, mediaTile, mediaUrl, mediaViewer, openMedia, openMediaExternally, previewMedia, type MediaScope, type MediaViewerFile } from "../../store/media";
import { MEDIA_TILE, mediaClock, mediaKind, mediaName, mediaNote } from "./media";
import { Icon } from "../lib/icons";
import { tr } from "../../i18n";

// Task 248: Qt's media previews (ActivityView mediaTiles: a 320 x 180 tile per
// picture, animation or video an agent names, its name on a dark strip and a
// play mark on a video) and its MediaViewer window (a picture or animation at
// up to 1280 x 860, or a player with Pause/Play, a position slider and the
// time; Open in default app; Close). The viewer is an in-app dialog.

function MediaTile(props: { scope: MediaScope; href: string }) {
  const [broken, setBroken] = createSignal(false);
  createEffect(() => previewMedia(props.scope, props.href));
  const tile = () => mediaTile(props.scope, props.href);
  const kind = () => tile()?.kind && tile()!.kind !== "none" ? tile()!.kind! : mediaKind(props.href);
  const url = () => tile()?.state === "ready" && tile()?.token ? mediaUrl(tile()!.token!) : "";
  const note = () => broken() ? tr("This picture cannot be shown.") : mediaNote(tile() as never, props.scope.machine);
  const name = () => tile()?.name || mediaName(props.href);
  return (
    <button class="media-tile" classList={{ video: kind() === "video", empty: !url() || broken() }} data-media={props.href}
      style={{ width: `${MEDIA_TILE.width}px`, height: `${MEDIA_TILE.height}px` }} title={props.href}
      aria-label={kind() === "video" ? tr("Play %1", name()) : tr("Open %1", name())} onClick={() => openMedia(props.scope, props.href)}>
      <Show when={url() && !broken()}>
        <Show when={kind() === "video"} fallback={<img src={url()} alt="" draggable={false} onError={() => setBroken(true)} />}>
          <video src={`${url()}#t=0.1`} muted playsinline preload="metadata" onError={() => setBroken(true)} />
        </Show>
      </Show>
      <Show when={kind() === "video"}><span class="media-play" aria-hidden="true"><svg viewBox="0 0 48 48" width="44" height="44"><path d="M18 12L36 24L18 36Z" /></svg></span></Show>
      <Show when={note()}><span class="media-note">{note()}</span></Show>
      <span class="media-name ell">{name()}</span>
    </button>
  );
}

/** The previews under a message or a tool group. */
export function MediaStrip(props: { scope: MediaScope | undefined; refs: readonly string[] }) {
  return (
    <Show when={props.scope && props.refs.length}>
      <div class="media-strip"><For each={props.refs}>{(href) => <MediaTile scope={props.scope!} href={href} />}</For></div>
    </Show>
  );
}

function Player(props: { file: MediaViewerFile }) {
  let video: HTMLVideoElement | undefined;
  const [playing, setPlaying] = createSignal(false);
  const [position, setPosition] = createSignal(0);
  const [duration, setDuration] = createSignal(0);
  const [error, setError] = createSignal("");
  onCleanup(() => { video?.pause(); video?.removeAttribute("src"); video?.load(); });
  return (
    <div class="media-player">
      <video ref={video} class="media-video" src={props.file.url} autoplay playsinline
        onPlaying={() => setPlaying(true)} onPause={() => setPlaying(false)} onEnded={() => setPlaying(false)}
        onLoadedMetadata={(e) => setDuration(Number.isFinite(e.currentTarget.duration) ? e.currentTarget.duration * 1000 : 0)}
        onTimeUpdate={(e) => setPosition(e.currentTarget.currentTime * 1000)}
        onError={(e) => setError(e.currentTarget.error?.message || tr("This video cannot be played."))} />
      <div class="media-controls">
        <button class="btn small" data-media-play onClick={() => { if (!video) return; if (video.paused) void video.play().catch(() => {}); else video.pause(); }}>{playing() ? tr("Pause") : tr("Play")}</button>
        <input type="range" class="media-position" aria-label={tr("Seek")} min={0} max={Math.max(1, Math.round(duration()))} value={Math.round(position())}
          onInput={(e) => { if (video) video.currentTime = Number(e.currentTarget.value) / 1000; }} />
        <span class="media-time tabular">{error() || `${mediaClock(position())} / ${mediaClock(duration())}`}</span>
      </div>
    </div>
  );
}

/** Qt's MediaViewer as an in-app dialog. */
export function MediaViewerDialog() {
  return (
    <Show when={mediaViewer()} keyed>
      {(file) => {
        let dialog: HTMLDialogElement | undefined;
        const [broken, setBroken] = createSignal(false);
        const previous = document.activeElement;
        onMount(() => dialog?.showModal());
        onCleanup(() => { dialog?.close(); if (previous instanceof HTMLElement && previous.isConnected) previous.focus(); });
        const title = () => tr("%1 — Zerus", file.name);
        return (
          <dialog class="dlg media-viewer" ref={dialog} aria-label={title()} data-kind={file.kind}
            onCancel={(event) => { event.preventDefault(); closeMediaViewer(); }} onClose={() => { if (mediaViewer() === file) closeMediaViewer(); }}>
            <div class="dlg-head"><Icon name={file.kind === "video" ? "video" : "image"} size={18} class="accent" /><h2 class="ell">{title()}</h2><span class="grow" />
              <button class="icon-btn" aria-label={tr("Close")} onClick={closeMediaViewer}><Icon name="x" size={16} /></button></div>
            <div class="dlg-body media-body">
              <Show when={file.kind === "video"} fallback={
                <Show when={!broken()} fallback={<div class="media-broken">{tr("This picture cannot be shown.")}</div>}>
                  <img class="media-picture" src={file.url} alt={file.name} draggable={false} onError={() => setBroken(true)} />
                </Show>
              }><Player file={file} /></Show>
            </div>
            <div class="dlg-foot">
              <span class="grow" />
              <button class="btn" data-media-external onClick={() => openMediaExternally(file)}>{tr("Open in default app")}</button>
              <button class="btn primary" onClick={closeMediaViewer}>{tr("Close")}</button>
            </div>
          </dialog>
        );
      }}
    </Show>
  );
}

/** Qt's "Video message" dialog (ActivityView::playVoice): the round video at a larger size with Play / pause. */
export function VideoMessageDialog(props: { url: string; onClose: () => void }) {
  let dialog: HTMLDialogElement | undefined, video: HTMLVideoElement | undefined;
  const [position, setPosition] = createSignal(0);
  const [duration, setDuration] = createSignal(0);
  onMount(() => { dialog?.showModal(); void video?.play().catch(() => {}); });
  onCleanup(() => { video?.pause(); dialog?.close(); });
  const ratio = () => duration() > 0 ? Math.min(1, position() / duration()) : 0;
  return (
    <dialog class="dlg video-message" ref={dialog} aria-label={tr("Video message")} onCancel={(event) => { event.preventDefault(); props.onClose(); }} onClose={props.onClose}>
      <div class="dlg-head"><h2>{tr("Video message")}</h2><span class="grow" /><button class="icon-btn" aria-label={tr("Close")} onClick={props.onClose}><Icon name="x" size={16} /></button></div>
      <div class="dlg-body video-message-body">
        <div class="video-message-round" style={{ "--p": String(ratio()) }}>
          <video ref={video} src={props.url} playsinline onLoadedMetadata={(e) => setDuration(e.currentTarget.duration * 1000)} onTimeUpdate={(e) => setPosition(e.currentTarget.currentTime * 1000)} />
          <svg class="vm-ring" viewBox="0 0 200 200" aria-hidden="true">
            <circle class="vm-ring-track" cx="100" cy="100" r="97" pathLength="100" />
            <circle class="vm-ring-done" cx="100" cy="100" r="97" pathLength="100" style={{ "stroke-dashoffset": String(100 - 100 * ratio()) }} />
          </svg>
        </div>
        <span class="tabular muted">{mediaClock(position())} / {mediaClock(duration())}</span>
        <button class="btn" data-video-toggle onClick={() => { if (!video) return; if (video.paused) void video.play().catch(() => {}); else video.pause(); }}>{tr("Play / pause")}</button>
      </div>
    </dialog>
  );
}
