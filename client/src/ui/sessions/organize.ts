import { batch, createSignal } from "solid-js";
import { listRows, type Row } from "./model";
import { catalog, moveRow } from "../../store/catalog";
import { multi, setMulti, selectedKey, projectOrder, setProjectOrder, toast } from "../../store/ui";

// Bulk selection and drag-and-drop organisation of the session list, after
// tray/src/SessionList.cpp (mousePressEvent, selectRange, placement, dropEvent).

// --- Bulk selection --------------------------------------------------------

let anchor: string | undefined;
const cardKeys = () => listRows().filter((r) => r.type === "card").map((r) => r.id);

export const multiCount = () => Object.values(multi).filter(Boolean).length;
export const clearMulti = () => batch(() => { for (const k of Object.keys(multi)) if (multi[k]) setMulti(k, false); });
export const setAnchor = (key: string) => { anchor = key; };

/** Ctrl/Cmd toggles one card; Shift selects the range from the anchor (with
 *  Ctrl/Cmd, added to the current selection). The open conversation stays. */
export function bulkSelect(key: string, additive: boolean, range: boolean) {
  batch(() => {
    if (range) {
      const keys = cardKeys();
      const from = anchor && keys.includes(anchor) ? anchor : selectedKey() ?? key;
      const j = keys.indexOf(key);
      const i = keys.indexOf(from) < 0 ? j : keys.indexOf(from);
      const span = new Set(keys.slice(Math.min(i, j), Math.max(i, j) + 1));
      if (!additive) for (const k of Object.keys(multi)) if (multi[k] && !span.has(k)) setMulti(k, false);
      span.forEach((k) => setMulti(k, true));
      anchor = from;
      return;
    }
    // A toggle starts from the open conversation, which is already selected.
    const open = selectedKey();
    if (!multiCount() && open && open !== key) setMulti(open, true);
    setMulti(key, !multi[key]);
    anchor = key;
  });
}

// --- Drag and drop ---------------------------------------------------------

export type DragItem = { kind: "session" | "group"; id: string };
/** Where a drop lands: a group (project) and the session or group it goes
 *  before (none = the end), drawn as a line or as a frame around a header. */
export type Drop = { group: string; before?: string; line?: number; inset?: number; box?: { top: number; height: number } };

export const SESSION_MIME = "application/x-zerus-session";
export const [dragItem, setDragItem] = createSignal<DragItem>();
export const [drop, setDrop] = createSignal<Drop>();
export const endDrag = () => batch(() => { setDragItem(undefined); setDrop(undefined); });

// The blank area after the list ends the last project (the listing names no default project yet).
const defaultProject = () => projectOrder().at(-1) ?? "";

/** SessionList::placement for a pointer at content offset `y` over row `overId`. */
export function placement(rows: Row[], offsets: number[], heights: number[], overId: string | undefined, y: number, item: DragItem): Drop {
  const end = rows.length ? offsets[rows.length - 1] + heights[rows.length - 1] : 0;
  let i = overId ? rows.findIndex((r) => r.id === overId) : -1;
  // The blank area after the list moves a session to the default project's end.
  if (i < 0) return item.kind === "group" ? { group: "", line: end, inset: 8 } : { group: defaultProject(), line: end, inset: 12 };
  // A subagent row is not a target of its own: the whole family is one session.
  while (i > 0 && rows[i].type === "child") i--;
  const over = rows[i];
  const group = over.type === "header" ? over.project : over.type === "card" ? over.session.project : over.parent.project;
  const top = offsets[i];
  let bottom = top + heights[i];
  if (item.kind === "session") for (let n = i + 1; n < rows.length && rows[n].type === "child"; n++) bottom = offsets[n] + heights[n];
  const nextHeader = () => { let n = i + 1; while (n < rows.length && rows[n].type !== "header") n++; return n; };
  if (item.kind === "group") {
    // The upper half of a header places before its group; anything else, after it.
    if (over.type === "header" && y <= (top + bottom) / 2) return { group, before: group, line: top, inset: 8 };
    const n = nextHeader();
    const next = rows[n];
    return next?.type === "header" ? { group, before: next.project, line: offsets[n], inset: 8 } : { group, line: end, inset: 8 };
  }
  // A session over a header moves into that project (frame around the band).
  if (over.type === "header") return { group, box: { top: top + 6, height: 30 } };
  if (y <= (top + bottom) / 2) return { group, before: over.id, line: top, inset: 12 };
  let n = i + 1;
  while (n < rows.length && rows[n].type === "child") n++;
  const next = rows[n];
  return { group, before: next?.type === "card" && next.session.project === group ? next.id : undefined, line: bottom, inset: 12 };
}

/** Applies a drop: reorders projects, or moves a session within or across projects. */
export function applyDrop(item: DragItem, d: Drop) {
  if (item.kind === "group") {
    if (d.before === item.id) return;
    const order = projectOrder().filter((id) => id !== item.id);
    const at = d.before ? order.indexOf(d.before) : -1;
    order.splice(at < 0 ? order.length : at, 0, item.id);
    setProjectOrder(order);
    return;
  }
  if (d.before === item.id) return;
  const moving = catalog.rows[item.id];
  if (!moving) return;
  if (moving.project !== d.group) {
    // Moving between projects is a guarded `hgs` action; this list only reorders.
    toast("Not moved", `Moving ${moving.title} to another project arrives with the session actions.`);
    return;
  }
  // No session after the drop point: after the project's last session.
  let before = d.before;
  if (!before) {
    const order = catalog.order;
    let last = -1;
    order.forEach((id, i) => { if (catalog.rows[id]?.project === d.group && id !== item.id) last = i; });
    before = last >= 0 ? order[last + 1] : undefined;
  }
  moveRow(item.id, before);
}
