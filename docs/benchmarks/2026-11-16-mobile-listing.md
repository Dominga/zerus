# Bounded full/mobile listing repair (zerus-iu768)

Base: `6d3bb6bfea9225cd822e3667676d2f6d09de6b11`. Qualification uses
private Arch state, isolated tmux and fake provider processes with genuine hook
caller ancestry. Each heavy command has its own shared lease, jobs/test threads
at most four. The installed application, connector and agents are outside this
qualification.

## Cause and admission decision

The mobile connector obtains a complete `hgs ls --json --local --no-archives`
body. The previous ordinary listing first required a manual display-cache opt-in;
its disabled-cache preflight refused a larger inventory. Once enabled, the live
projection unconditionally reread every canonical binding and shared a 16-file
limit. A genuine 20-session, 5.29-MiB fixture failed after 0.185 seconds with zero
stdout. Separating the refusal diagnostics confirmed the file limit, rather than
the original ten-second deadline.

The revised admission is explicit: **full listing canonical files: 16 to 256**,
using the existing indirect-observation entry ceiling. Explicit pages and their
deferred ownership proof retain 16. Both retain the selected canonical 16-MiB
byte limit, separate indirect 16-MiB/256-entry limit, original native deadlines,
ten-second response deadline and 64-MiB complete-body limit. Failed freshness,
byte, entry and time checks still refuse the entire response. A file ceiling does
not promise support for 256 arbitrarily heavy records.

Ordinary qualified Linux use initializes an absent cache policy automatically.
Explicit disable is durable even before the cache exists. Existing disabled,
malformed, unreadable or linked policy remains conservative. The coalesced worker
runs independently of recovery policy. macOS/Windows activation and watch readers
retain their separate scope.

## Public and canonical rows

V4 projections classify only known ordinary Claude/Codex public shapes. One held,
bounded complete ordinary-composer observation precedes the public normalization,
including idle rows. A phase shortcut cannot prove a missing no-hook panel.
Unknown frames, approvals, startup/private transitions, Kimi and native children
retain the original exact canonical enrichment and reconciliation. Public rows
never enter native card/admission or canonical reconciliation helpers. Their own
immutable native versions are checked again before publication. The original
request clock is checked before each row, before/after its public screen
precheck and before/after its public builder; an existing OS call may overrun
cooperatively, but no next public observation starts after expiry.

| Public fields | Source and parity control |
| --- | --- |
| Native terminal/name/ID/pane/run and current binding membership | Existing owner predicate and pinned generation; live ID/rename, different-run/pane and untracked-terminal controls |
| Phase/activity/process state, Background/Unknown and child status | Existing public display or original canonical builder; full mixed-session and Background/Unknown oracles |
| Reply/read/attention/question identity and completeness | Original journal/read-mark/decorator paths; full row oracle, native reply/approval and slow-provider controls |
| Action/fork/settings availability | Original public availability calculation or canonical enrichment; private classification, poisoned-display and forbidden public-card controls; effects retain independent canonical admission |
| Model/effort/account/project/parent/goal/quiet status | Existing public inputs/builders and final decorators; summary matrix, parent and current/legacy provider controls |
| Recovery user/success/cancel fields | Exact public values; complete recovery text up to 128 KiB, otherwise canonical selection; complete-row oracle |
| DeepSeek/OpenCode observations | Existing bounded current/legacy provider normalizers and original shared native clock |
| Coverage/ordering/body completeness | Pinned read-only generation, exact native versions, late writer refusal, full/page byte and file controls |

All accepted rows retain the existing public/canonical schema and status/reply
semantics. The intermediate inert-row proposal was withdrawn before production
implementation; it is excluded design evidence, not a shipped representation.

## Upgrade and reproducible controls

An already-complete/current-stamp v3 cache is rejected by the new reader, then
reprojected by bounded worker census into a complete v4 body without canonical
writes or agent restart. Resident old supervisors/providers can publish v3 when
they write later. Their original writer gate spans intent, staged publication and
settlement; the new worker reobserves the changed native version after release.
A no-op callback cannot mint a staged publication witness. The separately labelled
unchanged-stamp Store API component also verifies immutable snapshot protection
and eventual v4 convergence; it is not claimed as ordinary no-op provenance.

The maintained controls are in `tests/test_listing_index.py` and
`src/state/listing_metadata/tests/mobile.rs`. The CLI suite owns private
HOME/config/state/socket and clears inherited agent context. Select a locally
built test binary with `HGS_TEST_BIN`, set `PYTHONPATH=tests`, and run
`python3 -m unittest -v test_listing_index`. The optional
`ZERUS_LISTING_LIVE_COUNT=141` control measures a real working/tool-like
141-process corpus. The 94-process mixed fixture matches the owner diagnostic's
agent/phase dimensions using synthetic records and text, not copied owner data.

The connector's exact captured-body control lives in
`encrypted_runtime::tests::listing_control`. It runs the production native pipe
cap, snapshot augmentation, encryption, private HTTP receiver and phone opening.
It retains the original 8-MiB stdout, 1-MiB plaintext/peer snapshot and 2-MiB posted
ciphertext bounds, including an over-limit refusal control. Final measured
numbers, source/artifact seals, counterfactuals and original failed campaigns are
retained in the author qualification report. These fixture results do not claim
that the owner's original connector has already republished successfully.
