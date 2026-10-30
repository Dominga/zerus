import { createMemo, createSignal } from "solid-js";
import { createStore, produce, reconcile } from "solid-js/store";

export const ROW_DENSITY_KEY = "workspace/collapsedSessionRows";
export const MAX_COMPACT_ROWS = 50_000;
type Keys = readonly [string, string];

export interface DensityPort {
  stored(): unknown;
  ready: Promise<unknown>;
  save(keys: string[]): Promise<boolean>;
  keys(row: string): Keys | undefined;
}

/** The existing settings writer stores only fixed-size Qt-compatible hashes.
 * Rows without verified keys keep a bounded in-memory choice (browser/replay).
 * Existing explicit false/empty settings always win over an imported family. */
export function createRowDensity(port: DensityPort) {
  const [overrides, setOverride] = createStore<Record<string, boolean>>({});
  const touched = new Set<string>();
  const adopting = new Set<string>();
  const [cleared, setCleared] = createSignal(false);
  const saved = createMemo(() => {
    const raw = port.stored();
    return new Set(Array.isArray(raw) && raw.length <= MAX_COMPACT_ROWS
      ? raw.filter((s): s is string => typeof s === "string" && /^[0-9a-f]{64}$/u.test(s)) : []);
  });
  const valid = (keys: Keys | undefined): keys is Keys => Array.isArray(keys) && keys.length === 2
    && keys.every((s) => typeof s === "string" && /^[0-9a-f]{64}$/u.test(s));
  const persist = (keys: Set<string>) => { void port.save([...keys].sort()).catch(() => {}); };

  // Qt upgrades a conversation-less key when the exact identity arrives.
  // Only the observed row is considered; unloaded rows are never inferred.
  function adopt(keys: Keys) {
    if (keys[0] === keys[1] || adopting.has(keys[1]) || adopting.size >= MAX_COMPACT_ROWS) return;
    adopting.add(keys[1]);
    void port.ready.then(() => {
      const values = new Set(saved());
      if (!cleared() && values.delete(keys[1])) { values.add(keys[0]); persist(values); }
    // One attempt per observed fallback in this viewer. A refused settings
    // write must not become a retry loop driven by reactive rendering.
    }).catch(() => {});
  }
  function get(row: string): boolean {
    const override = overrides[row];
    if (override !== undefined) return override;
    if (cleared()) return false;
    const keys = port.keys(row);
    if (!valid(keys)) return false;
    if (saved().has(keys[0])) return true;
    if (saved().has(keys[1])) { adopt(keys); return true; }
    return false;
  }
  function set(row: string, compact: boolean) {
    if (!touched.has(row)) {
      if (touched.size >= MAX_COMPACT_ROWS) return;
      touched.add(row);
    }
    setOverride(row, compact);
    const keys = port.keys(row);
    if (!valid(keys)) return;
    void port.ready.then(() => {
      const values = new Set(saved());
      values.delete(keys[1]);
      if (compact && !values.has(keys[0]) && values.size >= MAX_COMPACT_ROWS) {
        setOverride(produce((o) => { delete o[row]; }));
        touched.delete(row);
        return;
      }
      if (compact) values.add(keys[0]);
      else values.delete(keys[0]);
      persist(values);
    }).catch(() => {});
  }
  function expandAll() {
    setOverride(reconcile({}));
    touched.clear();
    setCleared(true);
    void port.ready.then(() => { persist(new Set()); setCleared(false); }).catch(() => {});
  }
  const any = () => Object.values(overrides).some(Boolean) || (!cleared() && saved().size > 0);
  return { get, set, expandAll, any };
}
