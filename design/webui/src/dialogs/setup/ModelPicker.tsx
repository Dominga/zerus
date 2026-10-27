import { For, Show, createMemo, createSignal, onCleanup } from "solid-js";
import { createStore } from "solid-js/store";
import { Icon } from "../../lib/icons";
import { openCodeModels, type OcModel } from "../../mock/setup-extra";

// OpenCode model picker (OpenCodeModelPicker.*): grouped by provider with a
// debounced search, an "All models" catalog that is requested on first use,
// favorites with one request per star click, and at most 500 rendered rows.
// An unavailable favorite is listed separately and can never be chosen.

const MAX_ROWS = 500;

const [favorites, setFavorites] = createStore<Record<string, boolean>>(
  Object.fromEntries(openCodeModels.filter((m) => m.favorite).map((m) => [m.id, true])),
);

function marks(m: OcModel): string {
  if (m.unavailable) return "Unavailable";
  return [m.isDefault && "Default", favorites[m.id] && "Favorite", m.free && "Free"].filter(Boolean).join(" · ");
}

export function OpenCodeModelPicker(props: { value: string; onChoose: (id: string) => void }) {
  const [query, setQuery] = createSignal("");
  const [debounced, setDebounced] = createSignal("");
  const [all, setAll] = createSignal(false);
  const [loadingAll, setLoadingAll] = createSignal(false);
  const [loadedAll, setLoadedAll] = createSignal(false);
  let t: ReturnType<typeof setTimeout> | undefined;
  onCleanup(() => clearTimeout(t));
  const onSearch = (v: string) => { setQuery(v); clearTimeout(t); t = setTimeout(() => setDebounced(v.trim().toLowerCase()), 180); };
  const toggleAll = () => {
    const next = !all();
    setAll(next);
    if (next && !loadedAll()) { setLoadingAll(true); setTimeout(() => { setLoadingAll(false); setLoadedAll(true); }, 700); }
  };

  const groups = createMemo(() => {
    const q = debounced();
    const pool = openCodeModels.filter((m) => !m.unavailable && (all() && loadedAll() ? true : m.short));
    const hits = pool.filter((m) => !q || m.name.toLowerCase().includes(q) || m.id.includes(q));
    const byProvider = new Map<string, OcModel[]>();
    for (const m of hits.slice(0, MAX_ROWS)) {
      const list = byProvider.get(m.provider) ?? [];
      list.push(m);
      byProvider.set(m.provider, list);
    }
    const out = [...byProvider.entries()].map(([label, models]) => ({ label: models[0].folder ? `${label} · This folder` : label, models, unavailable: false }));
    const gone = openCodeModels.filter((m) => m.unavailable && favorites[m.id] && (!q || m.name.toLowerCase().includes(q)));
    if (gone.length) out.push({ label: "Unavailable favorites", models: gone, unavailable: true });
    return { out, hidden: hits.length > MAX_ROWS, shortTruncated: !all() && openCodeModels.some((m) => !m.short && !m.unavailable) };
  });

  const notice = () =>
    all() && loadingAll() ? "Loading all models…"
    : groups().hidden ? `Showing the first ${MAX_ROWS} matches. Search to narrow the list.`
    : groups().shortTruncated ? "Some models are not in this short list. Use All models to search every model."
    : "";

  let list!: HTMLDivElement;
  const onKey = (e: KeyboardEvent) => {
    if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
    e.preventDefault();
    const rows = [...list.querySelectorAll<HTMLElement>(".ocm-row:not([aria-disabled=true])")];
    const i = rows.indexOf(document.activeElement as HTMLElement);
    rows[(i + (e.key === "ArrowDown" ? 1 : -1) + rows.length) % rows.length]?.focus();
  };

  return (
    <div class="ocm" onKeyDown={onKey}>
      <div class="row">
        <input class="field grow" type="search" placeholder="Search models" aria-label="Search models" maxLength={200} value={query()} onInput={(e) => onSearch(e.currentTarget.value)}
          onKeyDown={(e) => { if (e.key === "ArrowDown") { e.preventDefault(); list.querySelector<HTMLElement>(".ocm-row")?.focus(); } }} />
        <button class="btn" classList={{ on: all() }} aria-pressed={all()} data-tip="Search every model OpenCode offers" onClick={toggleAll}>All models</button>
      </div>
      <Show when={notice()}><div class="hint ocm-notice">{notice()}</div></Show>
      <div class="ocm-list" ref={list} role="listbox" aria-label="Models">
        <For each={groups().out} fallback={<div class="hint ocm-empty">No models match this search.</div>}>
          {(g) => (
            <div role="group" aria-label={g.label}>
              <div class="ocm-group">{g.label}</div>
              <For each={g.models}>
                {(m) => (
                  <div class="ocm-row" role="option" tabindex="0" aria-selected={props.value === m.id} aria-disabled={m.unavailable}
                    classList={{ on: props.value === m.id, gone: m.unavailable }}
                    title={m.unavailable ? `OpenCode does not offer ${m.id} here. Its star removes the favorite.` : m.id}
                    onClick={() => { if (!m.unavailable) props.onChoose(m.id); }}
                    onKeyDown={(e) => { if (e.key === "Enter" && !m.unavailable) { e.preventDefault(); props.onChoose(m.id); } }}>
                    <span class="ocm-name ell">{m.name}</span>
                    <span class="mono dim ell ocm-id">{m.id}</span>
                    <span class="grow" />
                    <span class="ocm-marks">{marks(m)}</span>
                    <button class="icon-btn small ocm-star" classList={{ on: !!favorites[m.id] }} aria-label={favorites[m.id] ? "Remove from favorites" : "Add to favorites"}
                      data-tip={favorites[m.id] ? "Remove from favorites" : "Add to favorites"}
                      onClick={(e) => { e.stopPropagation(); setFavorites(m.id, !favorites[m.id]); }}>
                      <Icon name="star" size={13} />
                    </button>
                  </div>
                )}
              </For>
            </div>
          )}
        </For>
      </div>
    </div>
  );
}

export const modelName = (id: string) => openCodeModels.find((m) => m.id === id)?.name ?? id;
export const modelVariants = (id: string) => [...new Set(openCodeModels.find((m) => m.id === id)?.variants ?? [])].slice(0, 16);
