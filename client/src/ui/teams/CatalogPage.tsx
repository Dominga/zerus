import type { JSX } from "solid-js";
import { tr } from "../../i18n";
import { setPage } from "../../store/ui";
import { TeamCatalogEditor } from "./TeamCatalogEditor";

/** The catalog editor page (Qt's TeamCatalogEditor), reached from Settings >
 * Teams and from the Team designer. */
export function CatalogPage(): JSX.Element {
  return (
    <div class="catalog-page">
      <header class="nt-page-head">
        <h1>{tr("Team catalog")}</h1>
        <button type="button" class="btn ghost small" onClick={() => setPage("settings")}>{tr("Back to settings")}</button>
      </header>
      <TeamCatalogEditor onOpenDesigner={() => setPage("designer")} />
    </div>
  );
}
