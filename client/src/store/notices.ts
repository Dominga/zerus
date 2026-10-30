import { createSignal } from "solid-js";

// The window's one notice bus: the design's toasts in the bottom-right corner
// (design/webui, state.ts toast()). Action results, copies, updates and
// refusals all show here, for the design's time, and never over the header.
// Errors that need the reader stay until dismissed.

export interface Notice {
  id: number;
  tone: "error" | "warn" | "info";
  /** The toast's first line; action results carry only their text. */
  title?: string;
  text: string;
  sticky: boolean;
  /** Notices with one key replace each other, as one status line does. */
  key?: string;
}

/** At most this many show; the oldest non-sticky one goes first. */
export const MAX_NOTICES = 5;
/** The design's toast time. */
export const NOTICE_MS = 4200;

export interface NoticeBus {
  notices: () => Notice[];
  notify(tone: Notice["tone"], text: string, sticky?: boolean, key?: string, title?: string): number;
  dismiss(id: number): void;
  /** Dismisses the notice with this key, if one shows. */
  clearNotice(key: string): void;
}

export function createNoticeBus(): NoticeBus {
  const [list, setList] = createSignal<Notice[]>([]);
  let nextId = 1;

  function dismiss(id: number) { setList((all) => all.filter((n) => n.id !== id)); }
  function clearNotice(key: string) { setList((all) => all.filter((n) => n.key !== key)); }

  /**
   * Adds a notice and returns its id. A notice with a key first takes the
   * place of the one with that key; a passing one without takes the place of
   * the same passing message, as Qt's one notice line shows a repeat once
   * (several sessions updated together). Over the cap, the oldest other
   * non-sticky one goes first, then the oldest other; the new one stays.
   */
  function notify(tone: Notice["tone"], text: string, sticky = false, key?: string, title?: string): number {
    const id = nextId++;
    const replaced = (n: Notice) => key === undefined
      ? !sticky && n.key === undefined && !n.sticky && n.tone === tone && n.title === title && n.text === text
      : n.key === key;
    setList((all) => {
      const next = [...all.filter((n) => !replaced(n)), { id, tone, title, text, sticky, key }];
      while (next.length > MAX_NOTICES) {
        // Never the notice being added: it is the last entry.
        const others = next.slice(0, -1);
        const loose = others.findIndex((n) => !n.sticky);
        next.splice(loose < 0 ? 0 : loose, 1);
      }
      return next;
    });
    if (!sticky) setTimeout(() => dismiss(id), NOTICE_MS);
    return id;
  }

  return { notices: list, notify, dismiss, clearNotice };
}

/** The window's bus, shared by the action store and every toast. */
export const bus = createNoticeBus();
export const notices = bus.notices;
export const dismissNotice = bus.dismiss;

/** A titled notice, as the design's toast; an error that needs the reader can stay. */
export function toast(title: string, text: string, tone: Notice["tone"] = "info", sticky = false): number {
  return bus.notify(tone, text, sticky, undefined, title);
}
