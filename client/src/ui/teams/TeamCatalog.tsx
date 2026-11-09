import { For, Show, createMemo, createSignal } from "solid-js";
import type { JSX } from "solid-js";
import { tr } from "../../i18n";
import { Icon } from "../lib/icons";
import {
  applyConfirmation,
  proposalCounts,
  templateById,
  visibleTemplates,
  type Catalog,
  type CatalogProposal,
} from "./catalog";
import type { TeamOutcome } from "./types";
import "../styles/catalog.css";
import { TextInput } from "../lib/TextField";

export interface CatalogPort {
  decide(proposal: string, decision: "keep" | "reject" | "undo"): Promise<TeamOutcome>;
  apply(): Promise<TeamOutcome>;
  discard(): Promise<TeamOutcome>;
}

export interface TeamCatalogProps {
  catalog: Catalog;
  port: CatalogPort;
  onClose?: () => void;
}

export function TeamCatalog(props: TeamCatalogProps): JSX.Element {
  const [query, setQuery] = createSignal("");
  const [favouritesOnly, setFavouritesOnly] = createSignal(false);
  const [favourites, setFavourites] = createSignal<string[]>([]);
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal("");
  const [asking, setAsking] = createSignal<"apply" | "discard" | null>(null);
  const ids = createMemo(() =>
    visibleTemplates(props.catalog, {
      query: query(),
      favouritesOnly: favouritesOnly(),
      favourites: favourites(),
    }),
  );
  const counts = createMemo(() => proposalCounts(props.catalog));

  const decide = async (proposal: string, decision: "keep" | "reject" | "undo") => {
    setBusy(true);
    setError("");
    try {
      const outcome = await props.port.decide(proposal, decision);
      if (outcome.outcome !== "done") setError(outcome.outcome === "confirm" ? outcome.confirmation.text : outcome.detail);
    } finally {
      setBusy(false);
    }
  };
  const apply = async () => {
    setBusy(true);
    setError("");
    try {
      const outcome = await props.port.apply();
      if (outcome.outcome !== "done") setError(outcome.outcome === "confirm" ? outcome.confirmation.text : outcome.detail);
      else setAsking(null);
    } finally {
      setBusy(false);
    }
  };
  const discard = async () => {
    setBusy(true);
    setError("");
    try {
      const outcome = await props.port.discard();
      if (outcome.outcome !== "done") setError(outcome.outcome === "confirm" ? outcome.confirmation.text : outcome.detail);
      else setAsking(null);
    } finally {
      setBusy(false);
    }
  };
  const toggleFavourite = (id: string) => {
    const current = favourites();
    setFavourites(current.includes(id) ? current.filter((value) => value !== id) : [...current, id]);
  };
  const plate = (proposal: CatalogProposal) => (
    <div class={`cat-plate cat-${proposal.state}`}>
      <span class="cat-plate-title">{proposal.title || proposal.target || proposal.id}</span>
      <span class="cat-plate-meta">
        {proposal.kind}
        <Show when={proposal.action}> {proposal.action}</Show>
      </span>
      <span class="grow" />
      <Show when={proposal.state !== "kept"}>
        <button type="button" class="btn small" disabled={busy()} onClick={() => decide(proposal.id, "keep")}>
          {tr("Keep")}
        </button>
      </Show>
      <Show when={proposal.state !== "rejected"}>
        <button type="button" class="btn ghost small" disabled={busy()} onClick={() => decide(proposal.id, "reject")}>
          {tr("Reject")}
        </button>
      </Show>
      <Show when={proposal.state === "kept" || proposal.state === "rejected"}>
        <button type="button" class="btn ghost small" disabled={busy()} onClick={() => decide(proposal.id, "undo")}>
          {tr("Undo")}
        </button>
      </Show>
    </div>
  );

  return (
    <section class="cat" aria-label={tr("Team catalog")}>
      <header class="cat-head">
        <h2>{tr("Draft catalog")}</h2>
        <span class="cat-version">v{props.catalog.version}</span>
        <span class="cat-counts">
          {tr("%1 kept", counts().kept)}
          <span>{tr("%1 undecided", counts().undecided)}</span>
        </span>
        <span class="grow" />
        <button type="button" class="btn small" disabled={busy() || counts().kept === 0} onClick={() => setAsking("apply")}>
          {tr("Apply %1", counts().kept)}
        </button>
        <button type="button" class="btn ghost small" disabled={busy()} onClick={() => setAsking("discard")}>
          {tr("Discard draft")}
        </button>
        <Show when={props.onClose}>
          <button type="button" class="icon-btn small" aria-label={tr("Close")} onClick={props.onClose}>
            <Icon name="x" size={14} />
          </button>
        </Show>
      </header>

      <Show when={asking()}>
        <div class="cat-confirm" role="dialog" aria-label={tr("Apply the catalog draft?")}>
          <p>{asking() === "apply" ? applyConfirmation(props.catalog) : tr("Drop every proposal in the designer's draft? The catalog stays as it is.")}</p>
          <div class="cat-confirm-actions">
            <button type="button" class="btn ghost small" onClick={() => setAsking(null)}>
              {tr("Cancel")}
            </button>
            <button type="button" class="btn small" disabled={busy()} onClick={asking() === "apply" ? apply : discard}>
              {asking() === "apply" ? tr("Apply") : tr("Discard draft")}
            </button>
          </div>
        </div>
      </Show>

      <div class="cat-body">
        <aside class="cat-list">
          <div class="cat-search">
            <TextInput
              value={query()}
              placeholder={tr("Search templates or roles…")}
              onInput={(event) => setQuery(event.currentTarget.value)}
            />
            <button
              type="button"
              class="icon-btn small"
              aria-label={tr("Show favourites only")}
              classList={{ on: favouritesOnly() }}
              onClick={() => setFavouritesOnly(!favouritesOnly())}
            >
              ★
            </button>
          </div>
          <For each={ids()}>
            {(id) => {
              const template = templateById(props.catalog, id);
              return (
                <div class="cat-row">
                  <span class="cat-row-name">{template?.name ?? tr("No template")}</span>
                  <Show when={template}>
                    <span class="cat-row-meta">
                      {template!.roster.length} {tr("roles")}
                    </span>
                    <button
                      type="button"
                      class="icon-btn small cat-star"
                      classList={{ on: favourites().includes(id) }}
                      aria-label={tr("Favourite")}
                      onClick={() => toggleFavourite(id)}
                    >
                      ★
                    </button>
                  </Show>
                </div>
              );
            }}
          </For>
        </aside>
        <div class="cat-proposals">
          <div class="cat-proposals-head">
            <b>{tr("Proposals")}</b>
            <span class="grow" />
            <span class="cat-legend">{tr("Keep or reject each change, then Apply.")}</span>
          </div>
          <Show when={props.catalog.proposals.length === 0}>
            <p class="cat-empty">{tr("The designer has no proposals.")}</p>
          </Show>
          <For each={props.catalog.proposals}>{(proposal) => plate(proposal)}</For>
          <Show when={error()}>
            <p class="cat-error">{error()}</p>
          </Show>
        </div>
      </div>
    </section>
  );
}
