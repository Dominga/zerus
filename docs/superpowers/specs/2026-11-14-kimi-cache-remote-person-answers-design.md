# Person-authorized remote Kimi cache answers

Status: proposal for review, 2026-11-14. Tracking: `zerus-cdhjm.4`.
This note changes no command, authority or runtime behavior. Source baseline:
`373f717ef`, after the cache chooser recognition in PR #2186.

## Current boundary

`src/state/kimi_cache_hint.rs::available` refuses a local `agent_caller` and
checks the original pending input receipt, native process/pane/run/conversation
and complete chooser. `answer` delegates to the existing guarded native choice
transport. Observation does not press keys or replay the waiting input.

`src/cli.rs::remote_via` forwards ordinary `answer` over SSH without a verified
originating caller. `PERSON_ONLY_FORWARDED` deliberately contains only `task`,
`team` and `question-routing`; adding all `answer` commands would break generic
agent-usable questions. On the peer, missing agent markers do not establish that
a person selected one of Kimi's four cache choices. An SSH connection identifies
a configured computer/account, not a human gesture.

`src/state/questions.rs` already serializes answers by run, validates the exact
question/hash and answers, and reserves a durable request receipt before keys.
Its strict `AnswerRequest` rejects unknown fields. Neither a JSON `person: true`
field nor an environment/SSH marker may become authority.

## Proposed decision

Require a live, one-use native person approval specifically for source
`kimi_cache_hint`. Keep other answer sources and their current forwarding
semantics. Keep the command people use: `hgs @host answer SESSION --json` with
the existing answer body. It may ask the person to confirm in Zerus or on their
paired phone; an unattended CLI cannot manufacture that approval.

The receiving native authority must enforce this source-specific rule even when
the originating CLI is older or bypassed. `SSH_CONNECTION`, absent HGS markers,
an executable name or a claimed client type cannot select a less strict local
path. A local person using CLI can obtain the same native approval, or answer
in Terminal. A local Zerus/phone answer can carry its existing genuine person
interaction into this new admission after that issuer is qualified.

The guarantee concerns Zerus command and transport admission. It does not claim
to sandbox every process with the owner's OS privileges or authenticate a human
from process ancestry alone.

## Admission and delivery

1. The selected receiver provides a bounded read-only capability/prepare reply.
   It resolves the current question itself under the original run binding;
   client-supplied `source` is never decisive. If the exact question is not a
   cache chooser, keep its existing generic answer path. A question change
   invalidates preparation. Cache preparation sends no terminal input and
   neither consumes nor changes the original pending message receipt.
2. A cache preparation creates a short-lived receiver challenge containing its
   machine identity, canonical session/run/conversation, question ID/hash,
   original pending-input request/receipt identity, selected option ID, normalized
   answer digest and original answer request UUID. Bind the native process/pane
   birth identities and configuration to the receiver's retained context too.
   The chosen option must be exactly one of the four current native options;
   this is not approval for a different choice later.
3. Only a verified native person interaction can approve that challenge. The
   trusted review shows the computer, session, full current chooser/cost
   explanation and selected option; default is Cancel. Reuse the existing
   chooser's Submit when its native issuer is qualified, preserving its direct
   cost/choice flow. A CLI without that bound gesture requests the same review
   through a native confirmation. “Don't ask me again” explicitly names its
   future-warning effect. A phone approval comes through its authenticated
   paired connector and explicit Submit, never through a message, scheduled
   job, automatic approval or a renderer boolean.
4. The receiver's native custodian reserves that approval durably and returns an
   opaque handle for this exact answer attempt. A CLI can transport the handle;
   it cannot mint one. The issuer/verifier must use a reviewed native control
   registration and live origin/connection custody, or the authenticated paired
   device path. Ordinary SSH stdin, a private JSON file and same-UID socket
   credentials by themselves are insufficient proof of a person interaction.
   If an originating desktop needs to confirm across computers, its native
   issuer must first be explicitly trusted by the receiver; an existing SSH
   alias does not enroll such an issuer. Until that trust path is qualified,
   confirm through the receiver's trusted Zerus/phone or keep the card read-only.
5. Under the existing per-run answer lock, the receiver rechecks the exact
   pending input, question, process/pane/run/conversation and writable mode.
   Atomically bind consumption of the approved challenge to reservation of the
   original answer UUID/digest before releasing any navigation key. The native
   answer adapter then performs its current complete-frame checks again. A
   changed or unreadable binding refuses without choosing anything.
6. Keep submitted/answered receipts and uncertain delivery semantics. A repeated
   identical UUID can observe its recorded outcome; it cannot resend keys. An
   uncertain attempt fences the same question/pending-input identity even if a
   caller asks for another UUID, challenge or proof. Only a verified refusal
   before any keys permits a newly reviewed attempt. No timeout, disconnect,
   restarted GUI or changed caller may reconstruct approval or replay input.

The challenge admission and final answer are separate internal protocol phases;
they must not silently append fields to today's strict public AnswerRequest.
Specify a versioned private native route and capability marker during
implementation, while leaving the public body and API v1 receipt identities
compatible. Keep the existing receipt digest over the original public request
bytes; bind that digest and the normalized selection in the approval. Keep the native handle
outside that unchanged public body. A stripped handle never falls back to
generic cache delivery.

Use bounded native waits with one absolute deadline per phase. Proposed challenge
lifetime is at most two minutes, measured by the receiver; retention never renews
that deadline. Proposed limits are 64 live challenges total and eight per origin,
with indexed expiry and bounded pages. At capacity refuse new work; never evict a
live grant to admit another origin. Persist consumed/uncertain identity with the
existing bounded receipt/index policy. Restart invalidates unconsumed grants;
it does not discard or reopen uncertain attempts. These limits and the concrete
native registration protocol need review before implementation.

## What ppd26 can supply

Reuse the reviewed caller classification and route unwrapping from
`zerus-ppd26.9`: ordinary `@host`, reference hops, guarded-machine forms and
private command wrappers must agree before any SSH execution. For a cache
answer, a proven agent or marked/unproven hosted caller must be refused before
requesting person approval. Do not ban ordinary remote answers to other sources.
The receiver still enforces the positive person approval, so a sender that loses
its classification never gains cache authority.

The hosted-call work in ppd26 can supply native process/birth checks, host and
conversation binding, bounded proof exchange and explicit unknown/refused caller
states. Its per-call token proves an agent's tool-call origin; it is not a person
grant and must never be forwarded as one. Clearing markers, an unreadable or
reparented ancestry, an unmarked resident host and a failed proof are not positive
human evidence. Preserve the reviewed Under/Outside/Unverified distinctions and
refusal cases in ppd26.19/.21/.22. A generic command permitted without attribution
on Unverified does not thereby gain cache-choice approval.

The integration snapshot inspected at `1bb8c048731cd8cc4a954009b9bc09b88fa394c7`
exposes `Caller::{Person, Session, Refused}` in `src/teams/caller.rs`. Its own
contract explicitly says this is not verified Person authority: unmarked agents,
reparenting and some resident-host/process boundaries remain outside the interim
proof. Linux namespace/readability admission and macOS withheld-environment limits
must retain their reviewed refusals. `caller_hosted.rs` verifies the native host,
per-call conversation token and peer/birth identity within a two-second exchange;
its stated same-user token-copy limits also remain.

| Caller result | Cache answer admission |
| --- | --- |
| Session | Refuse the agent before forwarding or opening a confirmation. |
| Refused / unavailable | Refuse; a failed proof cannot become a person. |
| Person under the interim rule | May request native confirmation; cannot deliver without its separate positive approval. |

That integration is outside this baseline's ancestry. This design imports no
branch code or assumed qualification. Rebase implementation on the accepted
caller-provenance APIs and current platform review rather than copying historical
listener/token code. Native person issuance is an additional requirement, not a
rename of the caller classifier's Person result.

## Compatibility and qualification required before shipping

A new receiver requires approval for the cache source from all CLI versions.
A new sender must discover the receiver capability before attempting a cache
answer; an older/unknown receiver leaves it read-only with Open terminal, never
an unguarded fallback. An older sender can receive a truthful refusal from a new
receiver. Other question sources retain their existing request, answer and
receipt behavior. No global automatic-approval category, question routing rule,
implicit compact/resume or persistent permission is introduced.

Required isolated controls and production mutants:

- Local and remote person-approved answers for each of the four options; verify
  original input/answer receipts and the native acknowledgement/CID lineage.
  No answer selection is a new send, retyped message or automatic retry.
- Refuse local, forwarded, detached, hosted-tokenless and marked/unproven agent
  callers; exercise every route wrapper and marker clearing. Ordinary non-cache
  agent answers continue to work. Removing either receiving approval admission
  or origin-agent refusal must make its corresponding regression fail.
- Refuse forged, missing, expired, reused, wrong-machine/run/conversation,
  wrong-question/pending-receipt, changed-option/digest and wrong-issuer proofs.
  A JSON/client/env marker or copied hosted-call token never grants approval.
- Changed/clipped chooser, lost writable mode, process/pane replacement, native
  person cancellation and unavailable broker send zero keys and retain input.
  Challenge reads must be tested for zero navigation/Enter/model calls.
- Disconnect/crash after reservation or first key, duplicate UUID, concurrent
  different UUID and lost acknowledgement stay uncertain without replay. A
  mutant admitting a new challenge for the same uncertain question must fail.
- Old/new sender and receiver combinations, paired-phone authentication,
  genuinely selected desktop-origin registration, timeout/capacity/restart and
  live-grant retention need explicit qualification. There is no issuer key
  enrollment, native broker or phone-person proof implemented by this note.
- Qualify actual chooser delivery against one latest verified Kimi release in a
  private fixture, including a genuinely aged chooser when reproducible. The
  current synthetic screen and loopback tests do not establish that aged path;
  never inspect or answer an owner's live chooser to obtain qualification.
