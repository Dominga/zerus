/**
 * Draft keys (src-tauri `composer::session_draft_key`). Pure, so views that
 * compare a composer's key need no store.
 */
import type { SessionRef } from "../types";

const keyed = (s: SessionRef, part: string) => s.subagent
  ? `${s.computer}\n${part}\n${s.subagent.parent_conversation_id}\n${s.subagent.agent_id}`
  : s.archive_id ? `${s.computer}\n${part}\n${s.archive_id}` : `${s.computer}\n${part}`;
/** A canonical session ID (core `creation::canonical_uuid`): lowercase and not nil. */
const canonical = (id: string) => /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/.test(id) && /[1-9a-f]/.test(id);
const archivedRef = (s: SessionRef) => s.key.startsWith("archive:") || !!s.archive_id;

/**
 * Qt's main/archive and four-part child draft keys (src-tauri `session_draft_key`).
 * A live or saved session with a durable ID is named `id:<session_id>` in them,
 * so its drafts follow the session across a rename; an archived one keeps its name.
 */
export const draftKey = (s: SessionRef) =>
  keyed(s, s.session_id && canonical(s.session_id) && !archivedRef(s) ? `id:${s.session_id}` : s.name);
/** The name key a session's drafts had before its ID was known (src-tauri `legacy_draft_key`). */
export const nameDraftKey = (s: SessionRef) => keyed(s, s.name);
