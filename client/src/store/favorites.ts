import { createMemo, createSignal } from "solid-js";
import type { Session } from "../ui/sessions/types";
import { machineCards } from "./machines";
import { setSetting, settings, settingsReady } from "./settings";

export const FAVORITES = "sessions/favorites";
export const MAX_FAVORITES = 2048;
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/u;

/** A device-local preference, never a session action or an editable label. */
export function favoriteId(machine: string | undefined, session: string | undefined): string | undefined {
  return machine && session && UUID.test(machine) && UUID.test(session) ? `${machine}\n${session}` : undefined;
}

export function parseFavorites(raw: unknown): string[] {
  if (typeof raw !== "string" || raw.length > 256 * 1024) return [];
  try {
    const value: unknown = JSON.parse(raw);
    if (!Array.isArray(value)) return [];
    return [...new Set(value.filter((id): id is string => typeof id === "string"
      && favoriteId(...(id.split("\n") as [string, string])) === id))].slice(-MAX_FAVORITES);
  } catch { return []; }
}

/** Bounded optimistic choices, serialized through the existing settings writer. */
export function createFavorites(backend: { value: () => unknown; ready: Promise<unknown>; save: (value: string) => Promise<boolean> }) {
  const saved = createMemo(() => new Set(parseFavorites(backend.value())));
  const pending = new Map<string, boolean>();
  const [revision, change] = createSignal(0);
  let saving = false;
  async function flush() {
    if (saving) return;
    saving = true;
    try {
      // A click during startup must merge with the loaded preferences.
      await backend.ready;
      while (pending.size) {
        const snapshot = new Map(pending);
        const next = new Set(parseFavorites(backend.value()));
        for (const [id, on] of snapshot) { next.delete(id); if (on) next.add(id); }
        await backend.save(JSON.stringify([...next].slice(-MAX_FAVORITES)));
        for (const [id, on] of snapshot) if (pending.get(id) === on) pending.delete(id);
        change((n) => n + 1);
      }
    } catch (error) {
      pending.clear(); change((n) => n + 1);
      console.warn(`favorites: choice not saved: ${String(error)}`);
    } finally { saving = false; }
  }
  return {
    has(id: string | undefined): boolean {
      revision();
      return !!id && (pending.get(id) ?? saved().has(id));
    },
    set(id: string, on: boolean) {
      if (parseFavorites(JSON.stringify([id]))[0] !== id) return;
      pending.delete(id); pending.set(id, on);
      if (pending.size > MAX_FAVORITES) pending.delete(pending.keys().next().value!);
      change((n) => n + 1);
      void flush();
    },
  };
}

const store = createFavorites({ value: () => settings[FAVORITES], ready: settingsReady,
  save: (value) => setSetting(FAVORITES, value) });

export function favoriteIdentity(row: Session | undefined): string | undefined {
  return row ? favoriteId(machineCards[row.computer]?.machine_id, row.sessionId) : undefined;
}
export const favorite = (row: Session) => store.has(favoriteIdentity(row));
export function setFavorite(row: Session, on: boolean) {
  const id = favoriteIdentity(row);
  if (id) store.set(id, on);
}
