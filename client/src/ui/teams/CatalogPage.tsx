import { Show, createEffect, createSignal, onCleanup, type JSX } from "solid-js";
import { tr } from "../../i18n";
import { showCatalog, teams } from "../../store/teams";
import { catalogPort } from "../../store/teamsCatalog";
import { setPage } from "../../store/ui";
import { TeamCatalog } from "./TeamCatalog";
import { parseCatalog } from "./catalog";
import type { ComputerId } from "../../types";

/** The catalog page (Qt's TeamCatalogEditor), reached from Settings > Teams. */
export function CatalogPage(): JSX.Element {
  const [computer] = createSignal<ComputerId>("");
  createEffect(() => {
    const hide = showCatalog(computer());
    onCleanup(hide);
  });
  const catalog = () => {
    const view = teams.catalog(computer());
    return view ? parseCatalog(view) : null;
  };
  return (
    <div class="catalog-page">
      <header class="nt-page-head">
        <h1>{tr("Team catalog")}</h1>
        <button type="button" class="btn ghost small" onClick={() => setPage("settings")}>{tr("Back to settings")}</button>
      </header>
      <Show when={catalog()} fallback={<p class="dim">{tr("Reading the catalog…")}</p>}>
        {(shown) => <TeamCatalog catalog={shown()} port={catalogPort} onClose={() => setPage("settings")} />}
      </Show>
    </div>
  );
}
