import { For, Show, createSignal, onCleanup, onMount } from "solid-js";
import { Icon } from "../../lib/icons";
import { toast } from "../../state";

// The composer's attachments, after MessageComposer::rebuildAttachments: the
// chip opens a list; a row opens its file (click, Enter or Space), hovering or
// focusing it shows a larger preview beside the list, and only the row's ×
// (or Delete) removes it. Improvements over Qt: the preview also covers text
// files (first lines), removal can be undone from the list, and arrow keys move
// between rows. Synthetic files only.

export interface DraftAttachment {
  id: string;
  /** The marker the message text carries, as in Qt: "[Image #1]", "[File #2]". */
  reference: string;
  name: string;
  kind: "image" | "text";
  bytes: number;
  /** Image preview (synthetic SVG) or the first lines of a text file. */
  image?: string;
  excerpt?: string;
}

const svg = (body: string) => `data:image/svg+xml;utf8,${encodeURIComponent(`<svg xmlns="http://www.w3.org/2000/svg" width="320" height="200" viewBox="0 0 320 200">${body}</svg>`)}`;
// A synthetic screenshot: a window with a sidebar, a header and form fields.
const SCREENSHOT = svg(`<rect width="320" height="200" rx="8" fill="#1c2229"/><rect width="320" height="22" rx="8" fill="#242d36"/><circle cx="14" cy="11" r="4" fill="#ef7d6d"/><circle cx="28" cy="11" r="4" fill="#e8a75a"/><circle cx="42" cy="11" r="4" fill="#72cdb2"/><rect x="0" y="22" width="76" height="178" fill="#161b21"/><rect x="10" y="34" width="56" height="8" rx="4" fill="#233d35"/><rect x="10" y="50" width="48" height="6" rx="3" fill="#333c47"/><rect x="10" y="62" width="52" height="6" rx="3" fill="#333c47"/><rect x="92" y="36" width="140" height="10" rx="5" fill="#e8edf4" opacity=".85"/><rect x="92" y="60" width="210" height="26" rx="6" fill="#171d24" stroke="#333c47"/><rect x="92" y="96" width="210" height="26" rx="6" fill="#171d24" stroke="#333c47"/><rect x="92" y="132" width="98" height="26" rx="6" fill="#8bdfc0"/><rect x="200" y="132" width="102" height="26" rx="6" fill="#242d36"/>`);

export const SEED_ATTACHMENTS: DraftAttachment[] = [
  { id: "at1", reference: "[Image #1]", name: "checkout-form-layout.png", kind: "image", bytes: 245_760, image: SCREENSHOT },
  { id: "at2", reference: "[File #2]", name: "fixture-orders.json", kind: "text", bytes: 18_432,
    excerpt: `{\n  "orders": [\n    { "id": "o-1001", "total": 42.50, "cursor": "b3JkZXI6MTAwMQ" },\n    { "id": "o-1002", "total": 17.00, "cursor": "b3JkZXI6MTAwMg" },\n    { "id": "o-1003", "total": 99.90, "cursor": "b3JkZXI6MTAwMw" }\n  ],\n  "next": "b3JkZXI6MTAwMw"\n}` },
  { id: "at3", reference: "[File #3]", name: "pagination-notes.md", kind: "text", bytes: 3_120,
    excerpt: "# Cursor pagination\n\n- Keep v1 offset paging unchanged.\n- v2 returns `next` until the last page.\n- Page size is capped at 200." },
];

const kib = (bytes: number) => `${Math.max(1, Math.ceil(bytes / 1024))} KiB`;

export function AttachmentsChip(props: { items: DraftAttachment[]; onRemove: (id: string) => void; onRestore: (item: DraftAttachment, index: number) => void }) {
  const [open, setOpen] = createSignal(false);
  const [preview, setPreview] = createSignal<string>();
  const [removed, setRemoved] = createSignal<{ item: DraftAttachment; index: number }>();
  // The toolbar clips its overflow, so the list is placed in the viewport above the chip;
  // the preview goes to the right of the list, or to the left when there is no room.
  const [pos, setPos] = createSignal({ left: 0, bottom: 0, flip: false });
  const place = () => { const r = chip.getBoundingClientRect(); setPos({ left: Math.max(8, Math.min(r.left, innerWidth - 368)), bottom: innerHeight - r.top + 6, flip: r.left + 360 + 344 > innerWidth }); };
  let chip!: HTMLButtonElement;
  let list: HTMLDivElement | undefined;
  const close = (refocus = false) => { setOpen(false); setPreview(undefined); if (refocus) chip.focus(); };
  const rows = () => [...(list?.querySelectorAll<HTMLElement>(".att-row") ?? [])];
  const openFile = (a: DraftAttachment) => { close(); toast("Opening", `${a.name} opens in its default application on this computer.`); };
  const remove = (a: DraftAttachment, refocusIndex: number) => {
    const index = props.items.findIndex((x) => x.id === a.id);
    setRemoved({ item: a, index });
    setPreview(undefined);
    props.onRemove(a.id);
    // Keep focus in the list: the next row, else the previous, else Undo.
    queueMicrotask(() => { const r = rows(); (r[Math.min(refocusIndex, r.length - 1)] ?? list?.querySelector<HTMLElement>(".att-undo"))?.focus(); });
  };
  const onKey = (e: KeyboardEvent) => {
    if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); close(true); return; }
    const r = rows(), i = r.indexOf(document.activeElement as HTMLElement);
    if (e.key === "ArrowDown" || e.key === "ArrowUp") { e.preventDefault(); r[(i + (e.key === "ArrowDown" ? 1 : -1) + r.length) % r.length]?.focus(); }
  };
  const outside = (e: PointerEvent) => { if (open() && !list?.contains(e.target as Node) && !chip.contains(e.target as Node)) close(); };
  onMount(() => addEventListener("pointerdown", outside));
  onCleanup(() => removeEventListener("pointerdown", outside));
  const current = () => props.items.find((a) => a.id === preview());
  return (
    <span class="att-anchor">
      <button ref={chip} class="tchip" classList={{ on: open() }} hidden={!props.items.length && !open()} aria-haspopup="dialog" aria-expanded={open()}
        title={props.items.map((a) => `${a.reference} ${a.name}`).join("\n")}
        onClick={() => { setRemoved(undefined); place(); setOpen(!open()); if (!open()) return; queueMicrotask(() => rows()[0]?.focus()); }}>
        <Icon name="paperclip" size={13} />{props.items.length} attached
      </button>
      <Show when={open()}>
        <div class="att-pop" classList={{ flip: pos().flip }} ref={list} role="dialog" aria-label="Attachments" onKeyDown={onKey}
          style={{ left: `${pos().left}px`, bottom: `${pos().bottom}px` }}>
          <div class="att-head"><b>Attachments</b><span class="dim">{props.items.length} of 8 · up to 10 MiB each</span></div>
          <For each={props.items}>{(a, i) => (
            <div class="att-row" tabindex="0" role="button" aria-label={`Open ${a.name}`} title={`Open ${a.name}`}
              onMouseEnter={() => setPreview(a.id)} onMouseLeave={() => setPreview(undefined)} onFocus={() => setPreview(a.id)}
              onClick={() => openFile(a)}
              onKeyDown={(e) => {
                if (e.target !== e.currentTarget) return;
                if (e.key === "Enter" || e.key === " ") { e.preventDefault(); openFile(a); }
                else if (e.key === "Delete" || e.key === "Backspace") { e.preventDefault(); remove(a, i()); }
              }}>
              <span class="att-thumb" classList={{ file: a.kind !== "image" }}>
                <Show when={a.image} fallback={<Icon name="file-text" size={16} />}><img src={a.image} alt="" /></Show>
              </span>
              <span class="att-text">
                <span class="ell"><span class="att-ref">{a.reference}</span> {a.name}</span>
                <span class="dim tabular">{kib(a.bytes)}</span>
              </span>
              <button class="icon-btn small att-remove" aria-label={`Remove attachment ${a.name}`} data-tip="Remove (Delete)"
                onClick={(e) => { e.stopPropagation(); remove(a, i()); }}><Icon name="x" size={13} /></button>
            </div>
          )}</For>
          <Show when={removed()}>{(r) => (
            <div class="att-removed">
              <span class="ell">Removed {r().item.name}</span><span class="grow" />
              <button class="btn small ghost att-undo" onClick={() => { props.onRestore(r().item, r().index); setRemoved(undefined); }}>Undo</button>
            </div>
          )}</Show>
          <Show when={!props.items.length && !removed()}><div class="dim att-empty">No attachments.</div></Show>
          {/* The larger preview sits beside the list, like Qt's m_preview (320×240 at most). */}
          <Show when={current()}>{(a) => (
            <div class="att-preview" aria-hidden="true">
              <Show when={a().image} fallback={<pre class="mono">{a().excerpt}</pre>}><img src={a().image} alt="" /></Show>
              <div class="att-preview-cap"><span class="ell">{a().name}</span><span class="dim tabular">{kib(a().bytes)}</span></div>
            </div>
          )}</Show>
        </div>
      </Show>
    </span>
  );
}
