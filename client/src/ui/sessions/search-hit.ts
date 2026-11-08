import { tr } from "../../i18n";
import type { SearchHit } from "../../store/content-search-model";
import { buildTranscript } from "./transcript";
import type { TranscriptItem } from "./types";

/**
 * A shown search hit's message as Activity renders it (ActivityView.cpp:664,
 * 1090; zerus-5wmh.13.17.5): the hit's own event through the transcript, so
 * a reply is Markdown with links and Copy as Markdown. An excerpt of a
 * longer message is marked, and offers no copy of the whole reply.
 */
export function searchHitItems(hit: SearchHit): TranscriptItem[] {
  return buildTranscript([hit.event]).map((item) => (hit.content_truncated && item.kind !== "media" ? { ...item, excerpt: true } : item));
}

/**
 * The composer's jump button (ActivityView.cpp:1631-1636): it shows while
 * Activity is not following, and always while a search hit shows, when it
 * reads Back to latest activity.
 */
export function jumpButton(state: { following: boolean; unseen: number; searchShown: boolean }): { visible: boolean; caption: string } {
  if (state.searchShown) return { visible: true, caption: tr("Back to latest activity") };
  return {
    visible: !state.following,
    caption: state.unseen > 0 ? tr("Jump to latest activity (%1 new)", state.unseen) : tr("Jump to latest activity"),
  };
}
