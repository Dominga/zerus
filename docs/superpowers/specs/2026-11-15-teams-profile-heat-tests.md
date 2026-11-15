# Teams: Profile, Timeline, Heat, Tests and Builds

Design proposal for `zerus-63rru.173`, 2026-11-15. **No implementation or new
execution authority is approved by this note.** The Teams coordinator chooses
the slices. The governing contracts are [Agent Teams](2026-11-11-agent-teams-design.md),
[the current command authority table](../../reference.md#agent-teams), and the
[bounded desktop architecture](2026-10-27-tauri-web-ui-design.md).

## Source boundary and recommendation

Read against upstream main `06e9789737948a8f9f3cf0c5d6373327182fc5a7` (the Teams sources are unchanged
from the initial read at `ab1988f621879c5952dc00ac0a4b541482223b0b`).
WhiteWh's reference is `refs/remotes/whitewh-https/main` at
`ad0ad8d34181f270f4e890f853a8cbe9f0c3879a`, inspected with Git only. Its UI
ideas belong to WhiteWh; its `src/state/teams` engine and whole-team/profile
protocol are not ours. No fork executable, build or test was run.

Start with an **event timeline of one existing journal page**, projected in
client/core, with explicit page coverage and an optional export of that same
page. It is useful now, needs no new persistent fact or mutation, and must not
be advertised as measured work time. Follow it with a read-only projection of
existing effect/stage timings. Capture missing timing facts before offering
utilization or cause attribution. Baseline Heat and provider-backed Tests/Builds
need separate backend contracts; neither is a styling exercise.

The `.161` phase work is a dependency, not something this package implements.
The design/draft branch was read at `b531b48649114997dfcb114db857fbf559c7aa24`
and its next slice at `338753c96a302ef09b0be69099f1b1db26e0099f`.
Those branches have `DraftNode.estimate_minutes` and revisioned phase-draft
storage. They are **not in the audited main**. Recheck their landing state
before implementation; consume their IDs and generations rather than building
a second draft or estimate store. Main already has `Node.estimate_minutes`,
but ordinary queue intake writes `0`; `0` means no usable estimate. There is
no separate estimate field on main's `Phase`.

## Mode-to-engine map

| Fork surface | Required facts and behavior | What can be offered from our current contracts |
|---|---|---|
| Profile Time (`profile-page.tsx`, `TimeView.tsx`, `timeline.ts`) | Scope selection, member lanes, actual/planned paths, work/rework/wait/build spans, utilization, estimates, test runs | **UI/core only now:** chronological event marks for the selected journal page, actor IDs, recorded times and bounded counts. New backend **reads** can expose existing effect/stage timestamps. Full intervals, utilization and paths need the later facts below. |
| Profile Tokens | Attributed task/attempt tokens, roles, loss categories and partial-coverage flags | Not available from the Team store. Do not assign account/session totals to the currently selected task. Defer until task-attributed telemetry has its own source/coverage design. |
| Profile Members | Historical binding/model, attempts, accepted work, busy/idle/rework intervals | Current roster and existing bounded progress counts are displayable. Historical rates/utilization require binding snapshots and intervals; a current roster is not a historical lane. |
| Profile Compare | Two phase/window reads with comparable elapsed/work/wait/test measures | Once each underlying measure exists, compare like-for-like covered windows. No new mutation; do not compare an incomplete page with an entire phase as if equivalent. |
| Export (`profileExport.ts`) | Read snapshot plus optional spans/comparison saved as JSON | Export the **already loaded bounded scope** with schema, identity and coverage. Do not fetch an entire history to fill an export or copy the fork's protocol blindly. |
| Heat (`heat.ts`) | Frozen planned windows; actual milestones; own/inherited delay; own-work overrun; optional token loss | No baseline is stored today. Missing values stay unknown/untinted. A numeric estimate alone is not lateness. Baseline and observation slices are prerequisites. |
| Tests | Provider configuration, pending/running requests, full-suite policy/decisions, results and per-test reports | Existing pipeline **test stages** can be shown honestly as stage tasks/verdicts. They are not a provider-run database and are not proof of per-test coverage. |
| Builds | Build-provider identity, source binding, run status and artifacts | No Build stage kind or provider-run record exists. Do not relabel integration as a build or declare a successful build from task acceptance. Use the validation design below. |

The fork's Profile tabs are Time, Tokens, Members and Compare; its timeline
also contains test rows. Its heat distinguishes own lateness, inherited lateness
and estimate overrun, and offers a distinct token-loss mode. Preserve those
semantic distinctions when each is supported; do not make Tokens silently fall
back to lateness. Its test settings expose file/detected configuration,
overrides, providers, executable templates and acceptance hashes. Only the
presentation concepts are candidates for reuse.

## Facts we actually have

These are code-read findings, not claims from running live teams.

| Existing fact | Product writer/read path | Limit on its meaning |
|---|---|---|
| Journal `seq, at_ms, actor, kind, request_id, text, refs` | `ops/apply.rs::run` calls `ops/journal.rs::append` in the mutation transaction; `store/pages.rs::journal` reads live and archived journal rows | Host **recorded** time, not native start/end. Generic text is not a typed measurement. A failed/refused request is retained in the request receipt store; it need not have a successful-operation journal row. |
| Hand-out/reopen identity, node, member, generation, state/outcome, `created_ms` | `ops/handout.rs` reserves `effects`; outcome writes preserve the original effect | Reservation is not submission or work start. Refused, uncertain, cutting-over and superseded effects must remain distinct. |
| First recorded outcome time | `ops/timers.rs::outcome_time` writes `effect_outcomes(team,request_id,at_ms)` with `INSERT OR IGNORE` | Reuse this fact. It is when an outcome was recorded, including negative/unknown outcomes, not a native process duration. It is not currently a frontend profile read. |
| Stage wanted/created/finished state, `created_ms`, target/generation, verdict | `ops/pipeline/stages.rs`, `ops/stage_task.rs`, `store/pipeline-schema.sql`, `ops/pipeline.rs::verdict` | Stage creation is not its member starting work. Verdict records actor, pass and text, but no timestamp column or test counts; its successful operation has a journal timestamp. |
| Task observation and attempt correlation | `ops/types.rs::TaskObservation`, task observation companion rows and `ops/timers/attempts.rs` | Current/previous revision, stage, generation/attempt ID and conflict facts exist. They are not a durable archive of native work/idle/token intervals. |
| Inbox/card/integration timestamps | `inbox.created_ms`, `cards.created_ms`, `integration_facts.created_ms`, journaled decisions | Creation of a card is not the exact beginning of a wait; dismissal/reply alone does not prove native work resumed. Integration time is not compiler/build time. |
| Node estimate, dependencies, phase, role, pipeline, round | `crates/team-plan/src/model.rs::Node`; `ops/queue.rs::node_metadata` writes estimate `0` | Model support exists; usable estimates are not generally populated on main. Phase aggregate/critical path/ETA cannot treat omitted estimates as zero-duration work. |
| `.161` draft estimate and generation | At the pinned draft slice, `DraftNode`, `DraftNode::node`, `ops/drafts.rs` write `phase_draft_nodes` and generation records; estimates are bounded to one year | Proposed/branch facts. Drafts have no execution identity or dispatch authority. A sum of node estimates is not a phase's elapsed duration. |
| Current roster and queue-stage metadata | `cli/board.rs::queue`, core `view/board.rs`; test/review/land kind comes from the pinned pipeline | Main `StageKind` is Review/Test/Land. There is no Build variant. Queued, off-page and archived nodes need explicit coverage; absence from a page is not absence from the team. |

Current authority is stricter than aspirational passages of the original
specification: use the reference and `ops/authority.rs`. Journal/queue reads
are for the person and exact lead; members retain their current task/context
access. Service-only operations do not inherit Person authority. A stage
verdict comes only from its current, bound stage holder after generation,
round and pipeline checks; a lead or person must not manufacture that verdict.

## Data and projection rules shared by every slice

1. Use computer/home, team UUID, phase/node IDs, generation, attempt/effect ID,
   task UUID and the exact native binding where applicable. Labels identify no
   execution. A member swap can keep a member ID while changing its binding;
   historical lanes retain the binding they observed.
2. Store and expose wall times as integer **milliseconds**. The fork's chart
   adapter expects seconds; convert explicitly in one place. Order journal
   events by sequence even if the host clock moves backwards. Invalid time
   pairs produce an unknown duration with a reason, never a negative duration
   silently clamped into a trustworthy zero.
3. Keep metric name, source, unit, coverage and `as_of` together. Distinguish
   recorded elapsed, observed busy, reported outcome and measured process time.
   No native Stop, idle row, task acceptance or missing report proves that
   background jobs, tests or builds finished. Existing Background/attention
   precedence remains unchanged.
4. The existing journal cursor binds the team/list and a sequence ceiling;
   the read unions journal and archive, ordered by sequence, at most 200 rows.
   It excludes later insertions, **not mutations of other tables**. A page of
   events is not a complete-history sample. The decoder currently drops `refs`
   and feed generation, so richer links require an explicit bounded core
   projection; never parse the journal sentence to recover identity or state.
5. First UI slice retains one current event page, not an ever-growing journal
   or a per-team cache. Replace the page on Next/First; drop it on hide/team/
   computer change. Bind asynchronous reads to the selected identity/scope and
   request generation; discard late replies. A renamed team cannot retarget a
   reply. Existing journal reads start at the oldest page; do not label that
   page Latest without implementing a bounded indexed latest-window read.
6. New reads return scope, source revision/high-water marks, source-owned
   `as_of_ms` where used, coverage start/end, `more`/`truncated` and unknown
   reasons. Use indexed queries and bounded scans/joins. Never walk every
   journal page synchronously to paint a total, enumerate all sessions or load
   every task transcript. All-time aggregates need incrementally maintained,
   versioned aggregates or a separate bounded background job.
7. A currently open measured interval ends at a **verified source observation**,
   not an indefinitely advancing browser clock after service shutdown. Missing
   initial history, observer gaps, restart, conflicting attempts and old data
   remain visible as partial/unknown. Unavailable is not zero.
8. New writes follow `ops::apply`: verified current actor, strict envelope,
   request UUID, expected team revision, exact replay, stale refusal and journal
   in the same writer transaction. Service effects reserve their original
   identity before external work, preserve uncertainty and never repost it.
   A stale refusal keeps the editable form and original receipt; after refresh,
   a new explicit action gets a new UUID. An unknown action keeps its exact
   pending payload/UUID for Check, never an automatic repost. Neither is
   silently rewritten against a newer revision.

## Minimal backend additions

Names here are proposed internal operation/read names, **not advertised CLI
commands**. Prefer new modules/companion tables over changing old serialized
Node/Phase shapes. Pure display facts may be safely ignored by old readers;
mandatory validation/dispatch policy cannot be activated while an older actor
could bypass it. That slice needs an explicit capability/schema/open-handle
and native-admission compatibility fence, following `.161`'s executable gate.

### A. Existing timing read, then missing observations

First add a bounded `RecordedTimings` read joining effects, `effect_outcomes`,
stage records and their typed identities in one read snapshot. Expose the two
endpoints' provenance and coverage. Reservation-to-outcome is named
**delivery bookkeeping elapsed**, never member work. Pass/fail details can be
joined only where typed stage/attempt facts exist; do not scrape report text.
This is backend read work with **no new timestamp writer**.

For real Time/Members views add an append-only `TimingFact` companion:

- key: team/node/generation/attempt/effect, member binding incarnation and
  source observation ID; kind and producer schema version;
- opened/closed observation times, source event sequence, observer epoch,
  last verified observation and explicit gap/unknown reason;
- event-time phase/plan-generation and role/model/binding snapshot, so a later swap/rename does not
  rewrite historical membership;
- typed observations sufficient for the chosen metric: delivered assignment,
  observed native busy/background/attention, report, stage, decision/integration
  waits. Parallel tracks are separate; overlapping samples are unioned before
  any aggregate, not blindly added.

Only **service** records observed facts, under exact current binding and
source-generation checks (`RecordTimingFact`). Person/lead may read them,
never supply measured durations. Repeated/out-of-order observations are
idempotent and cannot replace a newer source. Restart opens an unknown gap;
there is no invented retrospective utilization. Persist meaningful transitions
and bounded/coalesced observation watermarks, not a new journal entry for
every render or watch tick. Preserve old timing facts for
old generations and keep late facts from changing a newer attempt's totals.
Define deterministic category precedence and minimum coverage before the UI
uses any Worked/Utilization/Top losses label. These are observed states, not CPU
accounting or evidence that a particular tool ran. Tokens need a further
attributed-usage fact contract and are deferred.

### B. Estimates and plan baseline

Reuse `.161`'s draft estimate and generation; do not invent an independently
editable phase estimate. Let person/exact lead edit an **unstarted draft**
through that design. Approval/baseline records freeze the estimate values and
the complete relevant draft/plan digest. Historical estimates do not follow a
later draft edit. `0`/missing estimates and off-scope prerequisites stay unknown.

Propose `FreezeBaseline` for the **person**, naming the exact reviewed plan,
source revision/draft generations, anchor time, node membership, hard deps,
barriers, order, estimates and baseline-policy version. Keep a complete
canonical digest plus a bounded frozen projection; no field whitelist that can
miss a scheduling input. A lead may propose it through the existing text-card path, not silently replace it.
The person reviews a recomputed typed preview; no baseline parameters are
recovered by parsing that card's words.
`SupersedeBaseline` is person-only and appends another version with a reason;
old comparisons keep the old version. This is analysis data, not dispatch
permission, task publication or a readiness change.

Recommended first policy for review: a **dependency-only baseline**, explicitly
labelled as such. Earliest start is the anchor or the latest known required
hard-predecessor/barrier finish; finish adds the known estimate. Any required
unknown makes the result unknown. It is not capacity-aware, does not invent
review/build estimates and must not be sold as a promised delivery schedule.
A queue is an open set: freeze only an explicit finite cohort; do not show a
standing team's completion ETA. Capacity-aware planning and pipeline-duration
estimates require their own policy before broader schedule claims.

First Heat compares the named actual milestone to the selected frozen window
with coverage shown. Overrun compares **like units/scopes**: an implementation
estimate with covered implementation observations, not acceptance time that
includes review and integration. Do not divide by zero or guess a role median.
Keep the fork's own/inherited delay fields **unknown** until a separately
specified causal model accounts for predecessor lateness, own waits and
resource contention. A dependency's lateness is not automatic proof of blame.
No baseline means untinted/unknown, not on time. No data ever means red solely
because a browser clock advanced.

### C. Tests/Builds providers and run records

The existing spec explicitly replaces repository shell templates with tasks
performed by member agents. This note preserves that boundary. Reading
`.zerus/tests.toml`, displaying a command or accepting a configuration digest
**never authorizes the service to execute it**. Automatic detection must not
run package managers, tests, builds or discovery scripts. A dedicated direct
runner, accepted executable templates, credentials, sandboxing, process-tree
custody/cancel and unknown-run recovery would need a separate owner-approved
execution design; copying the fork's test-config handler is not that design.

Minimal data contracts for an agent-task implementation:

| Record/read/op | Contents | Authority and effect |
|---|---|---|
| `ReadValidationConfig` / `CheckValidationConfig` | Versioned bounded project metadata; provider IDs and test/build kind; full/focused scope; report formats; origin file/detected/person override; exact source object/digest and diagnostics | Person/exact lead read; no execution or write. Detection proposes known metadata only. Follow native-selected project/checkout identity; regular-file/read bounds, no path traversal, unsupported schema fails closed. |
| `SetValidationConfig` | Reviewed immutable config version/digest, project and checkout scope, team policy, assigned role and limits | **Person** only; expected revision and UUID; journal the choice. Team creation/catalog import or a lead suggestion cannot silently grant new policy. Defaults request explicit review. |
| `RequestValidation` | Run UUID, provider/config version, full/focused selection, source commit/tree or verified checkout fingerprint, target task/node/generation/round/stage, origin and expected revision | Person or exact lead **within the configured policy**. Reserve once; service materializes an ordinary tester/build-agent task through the existing task/stage writers. Request is not execution proof. No member self-request grants broad project policy. |
| `RecordValidationTask` / `RecordValidationOutcome` | Original run/task/attempt/receipt identity, typed status and observed timestamps | **Service** only; existing hand-out, task observation and no-replay rules. Unknown remains Unknown; a fresh UUID cannot secretly resume an unknown run. |
| `SubmitValidationResult` | Exact current assigned attempt, reported status, full/focused claim, bounded counts/cases, artifact references and source/config correlation | Only that **member holder**, reusing current stage-verdict authority. Service settles the reported evidence against the existing binding. It is labelled member-reported, not independently measured. Wrong holder, old round/commit/config or missing identity cannot pass a gate. |
| Validation pages/detail | Pending/running/unknown/refused/cancelled/reported outcomes; partial per-case data, provenance and next cursor | Person/exact lead read; core projects bounded values and never chooses a verdict or starts/retries anything. |

A full-suite request is a distinct scope, not a synonym for any passing test
stage. If policy requires personal consent, open a person card bound to the
run UUID, source/config digest and expected revision. Lead can propose;
service requests/records the decision, never answers its own card. Approval,
rejection and defer are explicit; a timeout does not choose. Changing source or
config invalidates the selection and requires a new review, never an automatic
repost. No budget threshold or automatic full-run rule is imported by default.

Use existing Test stages for configured validation after an implementation;
keep current stage independence, holder, round, verdict and pipeline acceptance
rules. An agent that implemented the target cannot validate its own round.
When a person enables a mandatory validation policy, the existing verdict/
acceptance transition must check the exact required run, source, provider
version and full/focused coverage before advancing. A bare legacy `pass` boolean
cannot bypass that check; missing/partial/unknown results do not pass. Keep old
non-provider stages unchanged when that policy is absent, and fence older
writers before enabling the new gate.
Build is a validation **kind**, not a new `StageKind` enum value in old rows.
An initial build check can be described as such in an existing test/check task;
a dedicated Build pipeline primitive needs a separate compatible schema design.
A project-wide/full request without a target pipeline needs an explicitly
reserved check-task writer in this slice; do not pretend that API exists today.

Repository reports are untrusted data. A later result importer must bind the
chosen output to the same run/checkout, reject traversal/symlink escapes,
disable external entities/network access, bound bytes/cases/messages and record
parse failures/truncation. Display at most one result page (proposal: 200
cases), with separate total/retained counts; unknown totals are not zero.
Artifacts use existing product attachment/reference mechanisms, not arbitrary
renderer paths. A partial report cannot prove a full suite passed. Reuse is
initially off; future reuse needs identical source/config/provider/suite identity
and explicit provenance, never a status string or a matching branch name.

## Read-only client/core surface

Use new modules, scoped on-demand readers and the existing Teams frame hooks.
The first event page can consume `DrawerRead {kind: Feed}` and its
`DrawerPage {team,rows,after,next}`. Current feed rows expose recorded time,
actor, kind and bounded text. Task links need explicit core projection of
bounded canonical `refs` (they are currently dropped), or an existing
identity-bound selected task; no sentence parsing or off-page fleet scan.

Later `ProfilePage`/`TimingPage`/`ValidationPage` projections should include
capabilities and unsupported/unknown reasons. UI-only means **no new persistent
facts or operation authority**; it still permits a pure core adapter and tests.
A new backend read is marked separately above. Old/missing CLI capability keeps
advanced modes unavailable rather than showing an empty, supposedly successful
profile. Keep one selected scope/page; comparisons hold only two bounded
windows. Computation and I/O run off the GUI thread, with late-result fences.

Export versioned JSON of the selected covered window, including team/computer,
phase/cohort, source versions/high-water/as-of and partial flags. Use the
maintained native save picker/atomic data writer. No automatic whole-history
fetch, shell, config write, report submission, upload or live-session effect.
Do not include raw prompts/transcripts, credentials or arbitrary local files.
Render unsupported measures as unknown; do not synthesize the fork's full
payload with zeroes just to make its components render.

## Smallest safe slices, in order

Each row is a separate selectable PR, not authorization to implement it now.
Provider inspection can proceed independently of the baseline branch after the
first display slice; no need to ship misleading Heat while waiting for it.

| Slice | Backend/data and authority | UI delivered | Required proof before that slice ships |
|---|---|---|---|
| 1. Recorded event timeline | Existing journal/Feed read; pure core ordering/coverage adapter only | A lazy Timeline panel for one page, First/Next, recorded times and actor/kind; no work totals, heat or action buttons | Writer-produced journal and core fixtures, 201+ rows and fixed ceiling while new events arrive, live-to-archive move, clock reversal, empty/unavailable distinction, team/computer/selection race, bounded cache. Render/i18n/separators and no-write/no-launch assertions. |
| 2. Page export and existing timing read | Explicit data export; bounded read joining existing effect/outcome/stage facts, person/lead read guards | Export loaded page; recorded delivery/stage timing with precise labels and coverage | Original selected scope only; no background drain, malicious text/file name, byte bound, identity/revision mismatch, unknown/refused vs submitted distinction, joins through product writers and stable cursor while outcomes update. No statement of member work. |
| 3. Observation ledger | Service-only `RecordTimingFact`, append-only source identities and restart/gap facts; versioned bounded reads | Observed Time/Members windows; historical binding/role/model; unavailable metrics stay hidden/unknown | Duplicate/late observations, conflicting attempt, member swap and same label, old/new service restart, overlapping categories, clock discontinuity, terminal team, stale browser, page/archive/cap tests. No status/Background behavior change. Native fixtures only for any newly consumed native source. |
| 4. Draft estimate projection | Consume `.161` draft/approval generation and estimates; no parallel writer; approval history remains authoritative | Estimate versus covered actual in a bounded phase/cohort, explicit unknown and partial values | `.161` product draft writers, generation/edit races, estimate 0, over-bound values, off-page dependencies, no fabricated old estimates or automatic updates to historical comparisons. Approval remains unavailable until its own backend is ready. |
| 5. Frozen baseline and Heat | Person freeze/supersede ops, complete digest, explicit dependency-only policy; no scheduling changes | Reviewed baseline selection, planned versus observed milestones, conservative heat/overrun; causal own/inherited values remain unavailable | Authority/replay/stale, frozen cohort/graph/estimate changes, missing estimates, barriers/clock units, generation reuse, baseline supersession and queue growth. Mutants must kill zero-as-estimate, recaptured baseline and fabricated own delay. |
| 6. Provider inspection/settings | Bounded parse/check read and person config selection; no runner | Tests/Builds settings with source/digest/validation diagnostics and explicit unsupported capabilities | File/detected/override precedence, malformed/oversize/symlink files, unknown provider, changed checkout/config, actor matrix, old client/service compatibility. Merely opening/checking never executes or saves. |
| 7. Agent validation requests/results | Person/allowed lead request; service reserve/materialize/observe; current holder results; scoped personal full-suite decision where required | Pending/run/result pages and report detail, full-suite request/consent, honest reported provenance | Product task/stage writers and exact source/config/attempt binding; self-review refusal; crash cuts and lost replies; stale decisions; cancelled/unknown no replay; 201+ cases and report limits; partial/focused cannot satisfy full; no direct repo-command launch. Six or more runtime mutants at the new authority/identity/fallback production gates. |
| 8. Complete comparisons and optional telemetry | Pure comparisons only after compatible observations/coverage; attributed tokens and causal delay require separately approved source/model designs | Time/Members/Compare parity; Tokens/own-versus-inherited Heat only when data exists | Same cohort/window/units; partial versus full; no double-counted overlap; no token attribution by guess; older data/unsupported fields; export reflects exactly the selected version. |

For every implemented slice, first reproduce its negative control, use rows
from product writers (name hand-written corrupt fixtures), kill meaningful
production mutants, and qualify touched crates/UI locally under the shared
heavy wrapper. Regenerate Russian in `i18n.next-ru.json`/`i18n.ru.json` for new
runtime strings, preserve separator rules and lazy loading. Native qualification
is required when adding native source/lifecycle behavior, not for this document.
No deployment, installed-app change or gateway/phone migration is part of the
note or of the initial single-computer views.

## Decisions for the coordinator before backend implementation

- Select slice 1 first, or also authorize the independent provider-inspection
  branch. The recommendation is slice 1 alone as the smallest useful delivery.
- Confirm that the first timing labels are recorded/observed, and defer token
  loss, utilization and causal blame until coverage supports them.
- Choose/approve a baseline policy. The dependency-only proposal is deliberately
  limited; a capacity-aware calendar promise is a larger algorithm and UI.
- Keep Tests/Builds agent-task based under the existing spec, or request a new
  owner decision for a direct executable runner. Do not infer that decision
  from the fork or from this note.
- Choose when a full-suite request needs personal consent and the result
  assurance needed for a mandatory gate. Defaults here grant no automatic
  full-run/reuse policy and no execution permission from repository metadata.

## Code-read source index

- Main: `src/teams/ops/{apply,authority,journal,handout,timers,pipeline,stage_task}.rs`,
  `src/teams/ops/pipeline/stages.rs`; `src/teams/store/{schema,pipeline-schema,timers}.sql`,
  `src/teams/store/{records,pages,rows}.rs`; `src/teams/cli/{reads,board}.rs`.
- Kernel: `crates/team-plan/src/{model,catalog,pipeline}.rs`.
- Client: `client/core/src/teams/{read,drawers}.rs`,
  `client/core/src/teams/view/board.rs`, `client/src/ui/teams/feed.ts`.
- `.161` at the pinned branch refs: `docs/superpowers/specs/2026-11-15-teams-phases-m2.md`,
  `crates/team-plan/src/draft.rs`, `src/teams/ops/drafts.rs`,
  `src/teams/store/drafts.sql`. Their approval/execution parts remain separate.
- Fork UI only: `profile-page.tsx`, `TimeView.tsx`, `timeline.ts`, `heat.ts`,
  `testsSettings.ts`, `profileExport.ts`, plus `plan-panels.tsx` and the profile/
  timeline/test view types in `client/src/types.ts`. The fork's execution engine
  is not imported or used as an authority contract.
