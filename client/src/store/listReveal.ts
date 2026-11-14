import { createSignal } from "solid-js";
import type { Session } from "../ui/sessions/types";

export interface ListReveal { key: string; child?: string; row: string; serial: number }
export interface ArchiveReveal { session: Session; group: string; scope: string }
export const [listReveal, setListReveal] = createSignal<ListReveal>();
export const [listRevealPulse, setListRevealPulse] = createSignal<{ row: string; serial: number }>();
export const [listRevealNotice, setListRevealNotice] = createSignal<string>();
// One already-known selection, never a fetched page or growing archive cache.
export const [archiveReveal, setArchiveReveal] = createSignal<ArchiveReveal>();
let serial = 0;
export function requestListReveal(key: string, child?: string) {
  setListRevealPulse(undefined);
  setListReveal({ key, child, row: child ? `${key}/${child}` : key, serial: ++serial });
}
export function forgetListReveal() {
  setListReveal(undefined); setListRevealPulse(undefined); setListRevealNotice(undefined); setArchiveReveal(undefined);
}
