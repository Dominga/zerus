# Session listing field parity

This is the field/source/control audit for zerus-63r5s.2. The full-row controls
compare the canonical public builder with actual enabled ordinary `ls`, including
field presence, rather than comparing only a hand-picked summary. Watch
activation remains in .3. Display inputs grant no native action, caller,
confirmation, custody or cleanup authority.

The Python control names below are methods in `tests/test_listing_index.py`;
`summary_matrix` names the Rust
`tests::parity::public_terminal_summaries_match_the_canonical_builder` control.

| Public field | Source in the activated path | Full-row control |
| --- | --- | --- |
| `name`, `cmd`, `agent`, `backend`, `project`, `tag` | Same `listed_rows` / `label_row` builders, public seed and fresh terminal rows | `test_complete_saved_public_rows_cover_the_summary_and_private_transition_matrix`; `test_live_terminal_id_rename_reply_and_attention_keep_all_public_fields` |
| session/run/conversation/launch identity, ID layout and renamed label | Immutable generation seed plus exact native source version; same ID/label builders | `test_live_terminal_id_rename_reply_and_attention_keep_all_public_fields`; Rust `a_generation_row_never_borrows_a_later_sources_stamp` |
| `attached`, `clients`, `created` | Fresh bounded terminal listing and original saved-binding fallback | `test_live_terminal_id_rename_reply_and_attention_keep_all_public_fields` |
| activity/phase/conversation/runtime/process state, `activity_summary`, details and tool fields | Same `journal::normalized_activity`, `telemetry::summary`, active-tool metadata and bounded fresh canonical reconciliation | saved matrix; `test_background_unknown_and_recorded_jobs_match_the_canonical_live_rows`; `summary_matrix` |
| Child counts, groups, preview/completeness and `process_work` | Same `telemetry::children`, one fresh verified process batch; original Kimi roster and Claude internal-child normalization | saved matrix (`children`, Kimi-main/roster, Claude-internal, announced/external child); Background/Unknown control |
| Prompt, latest reply, reply identity, quiet/system-prompt filtering | Same journal summary, bounded native model history, original quiet/system filters | saved matrix; live reply/attention control; `summary_matrix` (`history`, `quiet`, `system-prompt`) |
| Attention signature, human-request flags, pending question and unread/reply fields | Same fresh read-marks/question/native route and notification overlays | live reply/attention control; `test_resident_deepseek_rows_keep_reply_and_person_attention`; Background/Unknown control |
| Model, effort, choices and pending Settings | Same `effort::summary` and bounded original native history/catalog/footer observations | saved matrix (`settings`); `summary_matrix` (`settings`, `history`) |
| Fork source/availability, provider errors/progress, account identity | Public seed plus same `fork::summary`, provider-error and account builders | saved matrix (`recipe`, `error`, `fork`, `provider-error`, `account`); `summary_matrix` |
| Action availability/reasons, restore/pause/resume blockers and private transition | Public predicates; fresh selected canonical bytes for private/native transitions (shared 16-file/16-MiB limit), never a reconstructed recipe | saved matrix (`transition`); `test_a_poisoned_display_completion_cannot_authorize_canonical_archival`; Rust `private_transition_data_is_not_cached_as_a_public_launch_recipe` |
| Goals, schedules, keep-warm, routing, preferences and notifications | Existing bounded enrichment functions after the same public builders | Complete row comparison in saved/live matrices (including default/absent values); original subsystem controls in the touched root modules |
| Parent link/state, account/project/folder placement | Same `parent_link`, workspace/account fields; indexed pinned parent lookup with aggregate 512-KiB budget | `test_parent_outside_the_requested_page_keeps_its_state`; saved/live ID/account/rename controls; Rust `parent_point_reads_share_one_query_byte_budget` |
| DeepSeek current/unavailable/legacy-compatible row, children, reply and person attention | Same resident snapshot normalizer, readonly bounded `list`; no binding inventory/adoption | `test_resident_deepseek_rows_keep_reply_and_person_attention`; `test_unavailable_native_and_terminal_rows_match_the_canonical_projection`; `test_slow_native_providers_keep_membership_with_one_bounded_observation_clock` |
| OpenCode current/legacy/unavailable row | Same service inspector/normalizer, bounded readonly HTTP; no adoption | `test_opencode_current_and_legacy_rows_use_the_same_readonly_normalizer`; unavailable and slow-provider controls |
| Archive preview/incomplete descriptor | Existing independent bounded archive reader; ordinary index does not supply archive authority | Existing archive/preview controls in the touched-module run; shared watch and archive-preview reader remain unchanged |

Raw shell output, tool arguments, transcripts, permission/confirmation tokens and
original custody are never copied into the display seed. A private transition
that cannot be represented by public predicates needs a fresh, bounded selected
canonical read; it cannot be reconstructed from display evidence.

Controls now compare complete public rows against the canonical builder, not
only summary fragments. `tests/test_listing_index.py` covers the terminal saved
matrix (including pending settings, quiet/system prompts, account/fork/errors,
private transition, Unknown, Kimi legacy-main/native-roster and Claude internal
children), live ID/rename/reply/attention rows, genuine private Background and
unknown-birth process trees, cross-page parents, a resident DeepSeek host and
current/legacy/unavailable OpenCode services. The twelve-case Rust summary
matrix additionally checks the bounded native model-history source. Observation
clock *presence and numeric type* are compared, while their values
(`goal_observed_at`, `process_work.sampled_at`) are allowed to advance, as in the
existing watch coalescer. All other public fields are compared unchanged.

Native child/process reconciliation uses a fresh selected canonical record.
Kimi records and unresolved working/finished child rosters share the existing
16-file/16-MiB selected-overlay limit with live/private transitions. A larger
request must use smaller pages or receives a truthful refusal; cached native
PID pins, launch recipes, confirmation tokens or removal outcomes never become
authority. Pure legacy Kimi-main/Claude-internal repairs can run on the bounded
public projection. Publishers still perform no native or journal I/O.
