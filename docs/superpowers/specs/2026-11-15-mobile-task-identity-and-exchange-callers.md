# Mobile task identity and exchange callers

Scope: zerus-oqh7.5 and zerus-oqh7.6, based on admitted PR #2462.

There is no protocol change from #2418. The existing `task_media_upload_v1`
capability selects the existing bounded `exchange_uploading` implementation;
`task_session_identity_v2` selects the already implemented Send/Done request
schema 2 on the compatible encrypted-v2 route. Cancel retains schema 1. The
relay, native admission, API v1, durable request identities and schema-8 task
execution controls do not change. Qualified schema-8 edits keep their separate
reviewed lane and frozen drafts from #2436/#2462.

## Immutable session presentation

Both phones carry the optional opaque session key through task selection,
routing, target pickers, highlighting and Done rows. Read native string
`launch_id` first, falling back to native string `run_id` only when launch is
absent or empty. Preserve bytes, including spaces; graph/session IDs and
string coercion never substitute for this key. A strong stored key resolves
only to a unique live session on the original connection and physical computer,
including after a rename. A reused label cannot satisfy a missing strong key.
Only a genuinely empty legacy key may use the existing label match. Current
labels are a read-only projection; changing labels never rewrites task identity.

Missing additive identity fields remain legacy; a present non-string field is
refused rather than silently converted to legacy. The captured host capability
map is indexed by the original connection and computer. An absent capability
or downlevel route selects today's schema-1 behavior before the request starts.
A schema-2 refusal or unknown result never causes a schema-1 retry.

## Scoped exchange evidence

Each exchange freezes its upload mode before the first page. Ordinary background
exchange still imports valid documents in a mixed page and preserves partial
exchange behavior. The UI can show a bounded diagnostic without loading a
history or retaining every page's refusal list.

Before Send/Cancel, the original selected task and project document must finish
without unsent or refused evidence on either side. Track only those required
keys across all pages, so a later clean page cannot erase an earlier refusal.
An unrelated damaged document does not block the selected valid task. Fetching
a particular task applies that same task/project check. Background exchange by
itself never grants native delivery authority.

Unknown Send holds release only after a fresh exchange proves their original
task/project against their original computer; settling one task cannot clear
an unrelated or concurrently replaced hold. Batch retained holds with a fixed
limit, preserving the rest for later document-sync passes. This performs reads
and document exchange only; it never repeats Send or reconstructs a request.

Host-advertised upload opt-in enables the existing exact hash/size/offset
framing and bounded private assembly. Old hosts keep the old exchange and get
a clear refusal when required media cannot fit it. No mid-exchange downgrade,
partial import, relay deployment or new endpoint is introduced.

## Local proof and release path

Use runtime RED controls for renamed/reused/foreign/duplicate/archived identity,
malformed Done keys, host-scoped negotiation, per-page task/project refusal,
unknown-hold retention and large-media upload opt-in. Retain legacy positives,
unrelated mixed-page progress and exact byte checks; kill production mutants
for each new acceptance guard. Qualify on Arch Android unit/lint/unsigned
assembly and a private Mac ZerusKit/unsigned simulator build. No hosted gate,
devices, signing or installation.

The maintained Android/iOS Dev paths already include these source trees. No
version, signing, distribution or gateway action is required for this slice;
the admitted native host capability and the next normal phone builds enable it.
