# Task-control writer / adapter census

Source inventory at main `6ef5d093c`, 2026-11-14. The current first PR declares the missing producer and keeps Begin/Yield planning refused; these writer/effect sites remain later slices. This inventory is not native runtime qualification.

| Family | Current source seam | Remaining control work |
| --- | --- | --- |
| CLI send / handoff | tasks/attempts.rs:438 final admit, :510 reserve, then deliver | Final executor slot/hold/provenance admission on owning computer; preserve C3, Teams guard and receiver exclusion |
| Message reopen / continue | tasks/attempts.rs:706 admit, :773 attempt plus note single save | Same immutable request and report origin; explicit reopen queues behind current work |
| Desktop Start / fresh executor | tasks/dispatch.rs:470 start-mode admit, :536 reserve, :695 native submit | Claim exact executor slot before owned input; startup conversation only via original scoped Begin |
| Teams handout | tasks/team_deliver.rs:853 admit, :1048 direct reserve | Separate direct reservation must participate too; preserve Service ownership and one original effect/receipt |
| Direct add --start / start / worker | tasks.rs:1086, :1296, :1297 | Separate direct-start provenance, no invented input receipt; preserve own acknowledgement/worker restrictions |
| Mobile selected CLI | tasks/mobile.rs::send -> common change path | No phone Person proof; explicit unsupported or separately qualified route; configured older CLI remains a gap |
| Native explicit interrupt | state/interrupt.rs::dispatch | Before effect publish durable Stop fact under native writer, then import after writer release; correlate task Pause separately |
| Verified external interrupt | state hooks/journal Interrupt evidence | Positive classified interruption only; ordinary provider Stop never becomes PersonStop |

Begin currently routes early from tasks.rs:592 to controls::dispatch before the ordinary task change/Teams guards. A future positive Begin cannot assume those guards already ran; it must preserve exact current task/executor acknowledgement rules and reject unsupported Team/child/native contexts. TaskDoc::begin_attempt checks exact delivered current attempt and executor, but that pure record method is not native actor proof.

Native identity surfaces: task-control Peer/Resident prove loaded hgs/resident process identity and bounded ancestry; observation::Observed exposes executor, agent, process and supervisor only. No task-current-turn witness is exported there. Codex rollout indexing has native turn_id/call_id; Kimi hook/wire has turnId/prompt evidence. Their presence is not a proof that an actual Begin peer belongs to that exact task input/current turn. Timestamp, prompt text and latest-record selection remain excluded. Missing or unqualified associations must refuse.

Existing observation explicitly refuses independently addressable children and shared/provider-host entries; native image/store guard exports remain Linux-qualified only. Mac peer mechanism qualification and Windows refusal-namespace controls do not fill these positive authority gaps. No current/live host, GUI, agent or connector was queried or changed by this census.

## Current refusal-only seam

`desktop/task_controls/native_turn.rs` declares an uninhabited, non-serializable
association type. There is no positive constructor, including in test fixtures.
The producer returns Unavailable; the consumer returns
`task_control_native_turn_association_unqualified`, `attempted: false`, and
`read_only: true`. It supplies no Plan, binding or durable Start-refusal proof.
The runtime consumes this result after its existing actual peer/CLI-command and
Person checks and before any task/control store access.

This path does not look up, write or acknowledge a task or open a Teams store.
It cannot replace ordinary task/Teams own-ack admission. Any future positive
handler must independently preserve those guards through its task save, as well
as supply the native association. No existing guard or API v1 prompt is relaxed.
With TASK_CONTROLS false, CLI admission and the unsupported Hello frame retain
their existing behavior and bytes.

The explicit `tests/task_control_producer_probe.py` suite requires an actual
separately built const-on binary; selecting an off binary fails its precondition.
It checks trusted CLI/service Begin and Yield-plan refusal, unchanged complete
private database contents, absent task/native/Teams stores with preview off,
Person guards before the producer gap, and rejection of serialized claims.
Ordinary off-build controls remain `test_task_control_service.InactiveBridge`
and `tests.test_task_controls.InactiveControlCommands`. A fixture does not
qualify a native producer.

A cooperating loaded adapter with synchronous input-generation revocation is a
separate architecture decision for the owner. Neither a terminal proxy nor a
tool wrapper is introduced here. TASK_CONTROLS remains false.
