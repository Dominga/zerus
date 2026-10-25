# External coding workstreams: ChatGPT Chat, Work and Codex Chat

This fork supports a cooperative workstream around a browser conversation. It is
not a consumer ChatGPT messaging API. A workstream records an explicitly selected
source, conversation URL, local workspace, bounded checkpoints and a durable inbox.
Native Codex and OpenCode sessions keep their existing transports and controls.
An external row never becomes a native provider merely because its model is Codex.

## Source and capability boundary

`kind` is one of `chatgpt_chat`, `chatgpt_work`, `codex_chat`. The desktop and
Android display distinct text badges; color is supplementary. These are explicit
operator declarations, not facts inferred from authentication, model names, MCP
metadata or a process parent. A normal native Codex row remains **Codex**. The optional adapter profile must
use the actual HGS host identity of the desktop/connector profile, not a guessed
phone display name.
Unknown/future metadata stays unknown.

The current URL contract is exactly `https://chatgpt.com/c/<UUID>` without query
parameters. It does not import a cloud transcript or accept arbitrary executable
links. A Codex CLI session should use the native adapter, not be registered as an
external browser chat. Other cloud-task URL formats require a separate adapter.

External capabilities are inspection, checkpoint history and a cooperative inbox.
There is no owned terminal, stop, resume, provider permission answer, fork or model
setting action. The explicit **Open originating chat** action opens only the
validated selected URL in the default browser; it does not send a prompt.

**Queue** saves a local instruction. The receipt says `delivery_state: queued`
and `provider_delivery: not_attempted`. The external agent must explicitly poll
using Desktop Commander and acknowledge it. An idle, closed, rate-limited or
context-exhausted chat is not woken. The implementation does not bypass ChatGPT
limits and must not be described as background cloud-chat control.

## Register one conversation

Use the same local HGS configuration/state as the desktop or outbound connector.
All examples are synthetic. Pass JSON through stdin, not inside a shell command:

```json
{
  "request_id": "11111111-2222-4333-8444-555555555555",
  "name": "external/sample/implementation",
  "kind": "chatgpt_chat",
  "url": "https://chatgpt.com/c/22222222-2222-4333-8444-555555555555",
  "cwd": "C:/Work/sample",
  "summary": "Verified starting checkpoint, with known limitations.",
  "next_steps": "Inspect the current source before changing it."
}
```

Command: `hgs external register --json`. Keep the returned `run_id` and
`conversation_id` with the workstream name. Never reuse the example request UUID
for another registration. The local store assigns a separate durable workstream
identity. Ordinary `hgs ls --local --json` includes its bounded projection.

The optional script in `romanilyin/DesktopCommanderMCP`,
`local/zerus-workstream.mjs`, validates the selected clean HGS build and invokes
this contract with literal argv and JSON stdin. It can be called by the existing
`start_process` tool without restarting the Desktop Commander server. See that
repository's `local/zerus-workstreams.md`. Keep machine profiles and tokens local.

## Checkpoint and cooperative inbox

`hgs inspect external/sample/implementation --skip-processes` returns the current
identity and checkpoint. `hgs external checkpoint --json` accepts:

```json
{
  "request_id": "33333333-2222-4333-8444-555555555555",
  "name": "external/sample/implementation",
  "expected_run_id": "COPY_THE_CURRENT_RUN_ID",
  "expected_conversation_id": "COPY_THE_CURRENT_CONVERSATION_ID",
  "summary": "What changed, exact commit and actual test results.",
  "next_steps": "The next concrete step and blockers.",
  "state": "ready"
}
```

Reported states are `working`, `ready`, `blocked`. They are agent reports, not
OS process observations; after five minutes the activity projection is unknown.
A checkpoint is bounded to 16 KiB per text field. Do not copy credentials or a
full proprietary transcript into status text.

`hgs send NAME --json` accepts the normal exact-scope request fields plus text;
attachments are refused. A successful receipt means **one local inbox record**,
not provider delivery. Request identities are durably bound to the complete
operation, target and payload. Repeating the exact request reconciles its stored
receipt; changing the payload under the same UUID is refused.

`hgs external inbox --json` reads the pending/accepted inbox with `name`,
`expected_run_id`, `expected_conversation_id`, optional `limit` (1–100) and `after`.
Reading does not acknowledge. `hgs external ack --json` adds a fresh `request_id`,
`message_id` and explicit `status`: queued -> accepted/rejected, then accepted ->
completed/failed. The endpoint performs no task execution. Agents still obey
current user instructions and tool permission boundaries; an inbox entry is not
an approval bypass.

Poll at safe boundaries during an active coding turn. Write a checkpoint after a
meaningful verified slice and before ending the turn. Keep uncertain-delivery
receipts; never retry with a fresh request UUID just to make an error disappear.

## Handoff after a chat/context limit

Resolve pending/accepted inbox work first. Then call `hgs external handoff --json`
with fresh request UUID, name, the **old expected** run/conversation IDs, a new
explicit conversation URL and its declared kind. The new run/conversation identity
is returned; the checkpoint and durable workstream identity survive. The old chat
is not stopped, renamed or modified.

Late old-chat sends and acknowledgements cannot silently target the replacement.
An outstanding instruction blocks handoff rather than migrating into a different
chat. Open the new chat yourself, provide its reference and inspect the checkpoint
through the same selected Desktop Commander/machine. This is state handoff, not
provider history cloning or automatic continuation.

## Storage and limits

The store is `HGS_STATE_DIR/external-workstreams/workstreams.sqlite3`, owner-private
and non-symlinked. Writes use SQLite transactions with a two-second admission
budget. Unknown schemas and redirected/private-state failures are refused, never
repaired by inspection. The database is capped at 131072 pages (normally 512 MiB).
No credentials are stored by this feature.

Inputs are at most 128 KiB; each instruction/checkpoint text is at most 16 KiB;
there are at most 128 queued/accepted instructions per active generation. Catalog
projection is bounded to 256 rows; `hgs external list --json` provides explicit
name-cursor pagination. Event and inbox pages are bounded and indexed, with
monotonic sequence cursors bound to the current run. The cloud transcript is
always unavailable. Older generations are retained as local audit state but are
not substituted as current conversation history.

## Phone control

A Windows connector admits an external row only with the exact cooperative
protocol/source metadata from a fresh local inspection. Only inspect/history/send
are admitted; archives, terminal, lifecycle and provider approvals remain denied.
A Linux connector can expose the same local workstream. Use a direct connector
for the environment that owns the state. Do not register the same state under
multiple computer credentials or copy its database across machines.

The Android fork is a separate package (`app.zerus.mobile.romanilyin`) and starts
with no default public relay. It displays the source and external-inbox warning.
Native OpenCode in WSL should use its own outbound Linux connector, not the
Windows gateway's unsupported one-hop OpenCode route.
