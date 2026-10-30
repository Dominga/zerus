import { createSignal } from "solid-js";
import type { SearchHit } from "./content-search-model";

/** Exactly one selected public excerpt; it never changes to the latest reply. */
export const [searchPreview, setSearchPreview] = createSignal<{ key: string; run: string; conversation: string; query: string; hit: SearchHit }>();
export const clearSearchPreview = () => setSearchPreview(undefined);
