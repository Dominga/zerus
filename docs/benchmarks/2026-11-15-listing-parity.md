# Session listing field parity

Qualification in progress for zerus-63r5s.2. This inventory is the review table;
controls and outcomes are filled from the source-bound local runs before the PR
is marked ready. Watch activation remains in .3. Display inputs grant no native
action, caller, confirmation, custody or cleanup authority.

| Public field | Existing source | Control |
| --- | --- | --- |
| `name`, `cmd`, `agent`, `backend`, `project`, `tag` | `state::listed_rows`, `label_row`; provider normalizers | terminal canonical-builder matrix; provider matrix |
| `session_id`, `run_id`, `conversation_id`, `launch_id` | canonical binding identity; native terminal IDs | ID-layout/rename fixture; native row-version controls |
| `attached`, `clients`, `created` | fresh terminal listing and binding fallback | native terminal merge fixture |
| `activity`, `phase`, `conversation_state`, `runtime_state`, `process_state` | `journal::normalized_activity` | canonical-builder matrix; stale/pinned reader controls |
| `activity_summary`, `activity_detail`, `current_tool`, `tool_detail` | `telemetry::summary`, active-tool metadata | children/quiet/system-prompt matrix |
| child counts, groups, previews and completeness | `telemetry::children` | children matrix; Background/Unknown fixture |
| `process_work` | one verified native process sample per listing batch | Background/Unknown fixture; recorded jobs do not establish work |
| `prompt`, latest reply and reply identity | `journal::user_prompt`, indexed `latest_reply` | quiet/system-prompt matrix; journal reply control |
| attention signature, pending questions and human-request flags | `read_marks`, native question readers, route outcomes | pending/unknown attention fixture |
| `model`, `effort`, available choices, pending settings | `effort::summary`, bounded native history/catalog/footer reads | settings/history matrix |
| fork source and availability | `fork::summary`, native launch-recipe presence | recipe/error/fork-origin matrix |
| action availability/reasons; restore/pause/resume blockers | `session_action::summary`, original canonical checks | private-transition evidence control; canonical source remains unchanged |
| provider errors and progress timestamps | `provider_errors::enrich` | provider-error matrix |
| goals, schedules, keep-warm and question-routing views | their existing bounded overlays | shared row/overlay control |
| read/unread state and notification preferences | `read_marks::enrich_listing`, `notifications::enrich` | shared row/overlay control |
| parent, account and project/folder placement | `parent_link`, `workspace`, account marks | parent/account/rename matrix |
| DeepSeek row, children and unknown host state | existing resident `list`/snapshot normalizer, unchanged compatibility | private fake-host/provider parity control |
| OpenCode current/legacy/unavailable row | existing service inspector and normalizer | private fake-service/provider parity control |
| archive preview and its incomplete descriptor | bounded canonical archive reader, independent of display index | existing archive preview controls |

Raw shell output, tool arguments, transcripts, permission/confirmation tokens and
original custody are never copied into the display seed. A private transition
that cannot be represented by public predicates needs a fresh, bounded selected
canonical read; it cannot be reconstructed from display evidence.
