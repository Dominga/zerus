import { batch, createEffect, createMemo, createRoot, createSignal } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import { inTauri } from "../bridge";
import { catalog, computerFresh, orderedRows } from "./catalog";
import { fleetComputers } from "./archive";
import { machineCards } from "./machines";
import { createContentSearch, type FoundHit, type SearchReply } from "./content-search-model";
import { setSearchPreview } from "./searchSelection";
import { openSession } from "./desk";
import { setDetailTab } from "./ui";
import { goToMessageRows, type GoToMessage } from "../ui/sessions/goTo";
import { tr } from "../i18n";

// Task 423: the title-bar search finds messages too. The palette has its own
// contents search (the panel's search keeps its own query and results); a
// palette query never cancels the panel's reads, it only drops its own stale
// replies by generation.

export const { goToMessageQuery, setGoToMessageQuery, goToMessages, goToMessagesBusy, openGoToMessage } = createRoot(() => {
  const search = createContentSearch({
    read: (computer, query, generation) => inTauri() ? invoke<SearchReply>("content_search", { computer, query, generation }) : Promise.reject(tr("Contents search needs the desktop app.")),
    cancel: () => {},
  });
  const [goToMessageQuery, setGoToMessageQuery] = createSignal("");
  const hosts = createMemo(() => fleetComputers(catalog.computers, orderedRows(), machineCards).map((host) => ({ ...host, online: computerFresh(host.computer) })));
  // Commands (">") and an empty field search no messages; a closed palette searches nothing.
  createEffect(() => {
    const query = goToMessageQuery().trim();
    search.configure(query.startsWith(">") ? "" : query, hosts(), query !== "" && !query.startsWith(">"));
  });
  const liveRow = (computer: string, hit: FoundHit["hit"]) =>
    (hit.run_id ? catalog.rows[`${computer}\nrun:${hit.run_id}`] : undefined) ?? catalog.rows[`${computer}\nname:${hit.name}`];
  const goToMessages = createMemo(() => goToMessageRows(search.hits(), liveRow));
  /** Opens the message's session at the message, as the panel's search does. */
  function openGoToMessage(message: GoToMessage) {
    const { row, found } = message;
    batch(() => {
      openSession({ computer: row.computer, key: row.catalogKey, preserveFilters: true });
      setDetailTab("activity");
      setSearchPreview({ key: row.key, run: row.runId ?? "", conversation: row.conversationId ?? "", query: goToMessageQuery().trim(), hit: found.hit });
    });
  }
  return { goToMessageQuery, setGoToMessageQuery, goToMessages, goToMessagesBusy: search.busy, openGoToMessage };
});
