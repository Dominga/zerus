# Mobile task boundary

`TaskStore` exposes the shared replica through generated Kotlin and Swift bindings.
Native Send and Cancel run on the selected computer; this library edits documents
and carries the bounded schema-1 exchange. Only `crates/replica` imports Loro.

`SessionRef.session_id` retains the opaque native identity. Its label is a hint;
empty identity denotes an older name-only link. Description media and identity use
task schema 2. This ABI still refuses every writer above schema 2, including
comment-media and executor-plan writers. Previews grant no mutation authority.

Task rows contain at most eight description-file metadata records, never file
bodies. Each history row has a stable entry ID and attachment count. `media`
fetches metadata for one description or one history entry, at most eight files.
`read_media` reads one selected file, at most 10 MiB, after checking its exact
task UUID, scope, position and token. The token seals project/task identity,
metadata and the SHA-256 of the returned bytes. It cannot be rebound to another
card, comment, position or changed file. Apps store preview bytes privately;
metadata names are display text, never filesystem authority.

`add_with_media` validates the complete selection before saving the task.
`edit_content` takes the original derived `content_revision`, retained file
tokens and new bounded byte inputs. It checks the revision under the existing
project lock and saves text and files together. Changed body/media requires
fresh review; unrelated history remains intact. Existing text-only edits keep
all file bytes. A new empty-media legacy card stays schema 1; removing files
never lowers a task's schema. The core's eight-file, 10 MiB/file, 20 MiB/task and
retained-history limits remain authoritative.

Phone drafts capture their original project and draft identity before a picker
or background read starts; edits and previews also capture the task UUID and
content revision. A delayed result must update only that draft, and must never
follow the currently selected card. Schema 1 reads,
record constructor defaults and the existing paged exchange remain compatible.
Rust view conversion is fallible so invalid media reports an error.

Large inbound documents already use bounded parts. The legacy outgoing exchange
reports a single update beyond its 512 KiB budget as `unsent`; no Send may follow
that result. Until the separately advertised upload extension is available,
pickers must refuse unsupported/unproven transfers upfront with a clear message.
Never drop bytes, fall back to text-only delivery or retry an unknown native Send.
