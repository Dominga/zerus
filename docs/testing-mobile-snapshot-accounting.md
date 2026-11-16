# Private mobile snapshot accounting

The ignored connector **library** control is an opt-in diagnostic, not a service,
owner probe, compatibility certificate or relay client. It measures the exact
public selected-only-field projection through the production Connector and Engine
serializer. The existing 1 MiB plaintext gate is unchanged.

Build on the qualified source under the shared heavy lease, clearing inherited
agent `HGS_*` context as for other tests:

```sh
cargo +1.99.1 test --manifest-path services/mobile/connector/Cargo.toml \
  --locked --lib --no-run -j 4
```

Use the `zerus_mobile_connector-*` library test executable printed by Cargo,
not the similarly named binary test target. This control is ignored by normal
test discovery and reads stdin only when selected explicitly.

The stdin object is:

```json
{
  "listing_json": "<one complete native ls --json --local --no-archives stdout>",
  "computer_name": "<the actual displayed computer name>",
  "binding": "<the existing connector binding hash, not its token>",
  "via": null
}
```

The coordinator supplies a complete current body and these noncredential
metadata in RAM, through stdin, a memfd or an anonymous pipe. Do not capture an
owner body/profile/token in a file, command log, artifact, test failure or Bead.
A stale/refused/partial listing is not an input. No actual workspace profile or
secret is accepted by the helper. The native CLI is selected explicitly:

```sh
python3 scripts/diagnostics/account-mobile-snapshot.py \
  --test-bin "$ACCOUNTING_LIBRARY_TEST" --native "$NATIVE_HGS" < /dev/fd/3
```

Here fd 3 is the caller-owned anonymous input containing the object above.
The wrapper disables core dumps, discards test-harness stderr and prints only
the one JSON aggregate report. It does not print its input or native responses.
Do not execute an unreviewed helper against owner data.

The first observation uses the chosen CLI for a fixed read-only allowlist:
help, mobile identity, two eligible selected inspections, archives, swarm get,
task digests and the account catalog. The supplied listing replaces only the
native ls response. It is never reconstructed from a partial cache. Existing
machine and selected-ID guards, native byte limits and call deadlines apply.
The responses stay in a bounded RAM replay table. The second observation uses
the exact six-key projection and replays those same auxiliary observations,
so unrelated native changes cannot masquerade as projection parity.

Both observations use the production `Connector::snapshot` augmentation:
archive preview, read-attention, project membership/digests, account catalog
and advertised capabilities. Public and augmented rows must match outside
exactly the six removed top-level keys; every identity must remain equal.
Engine computes the real scope fields and final computer wrapper, and uses
its production `encoded()` serializer. An **unarmed-by-default, test-only**
interception stops immediately after encoding, before the cap, authorization,
owner-bearing journal storage, any snapshot/signals HTTP, or alerts delivery.
Production builds do not contain that interception or stdin control.

The report includes raw/encoded input sizes; per-field present counts and
encoded savings; original/projected native, augmented and final-computer
sizes/hashes; exact final plaintext slack; identity/semantic equality; native
call counts and archive/project availability. If within the unchanged cap,
synthetic identities exercise the production crypto seal/open and envelope
serializer entirely in RAM. A deficit is a failed sufficiency condition, not
a reason to increase the cap or drop more data.

Differences from the original resident connector are explicit: fresh connector
caches and an initial account catalog (not the resident's older usage cache),
the supplied immutable listing instead of its current watch catalog, a fixed
comparison clock, a synthetic encryption workspace/keys and no real HTTP or
service configuration. Background account refresh is stopped with the private
observer; it cannot enlarge the already-returned initial snapshot. Supply the
actual computer name/binding/via to preserve those size inputs. Synthetic
UUIDs/HMAC scopes have the normal encoded lengths, but the synthetic ciphertext
header is not a byte certificate for the original membership history.

An accepted measurement proves only the given complete cut plus this real,
fresh augmentation fits. Root must still verify the original running service,
its warm extras, a new encrypted heartbeat and the installed phone after rollout.
Intermittent truthful stale-cut refusals remain a separate availability issue.

## Section accounting

The extended report measures current and archived rows separately at the final
Engine cut, with counts, encoded row sums, array sizes and maximum archive-row
size. It reports exact encoded property deltas for the sessions array, archive
metadata, projects, accounts and capabilities; archive loaded/total/partial,
availability and catalog/operation counts contain no owner strings.

`without_archive_rows_only_bytes/slack` is arithmetic on the existing encoded
array. It leaves archive metadata unchanged and is NOT a proposed deliverable
snapshot. It helps determine whether optional archive rows alone can explain a
deficit. An actual adaptive preview would need separately approved honest
metadata and a new final-encoding check. No such production adaptation occurs
in this helper or the six-field cleanup.

This diagnostic is for the local computer: supply `via: null`. Engine itself
derives its local `via` from the route/profile. The last snapshot argument is
presence used after the intercepted cut, not a routing override. Do not use
this helper to certify a remote/legacy peer packet or the resident warm caches.

## Exact public boundary

Only top-level session-row `recovery_user_text`, `recovery_user_at`,
`recovery_success_at`, `recovery_cancel_at`, `keep_warm_prompt_at` and
`model_options` are omitted, after all public derivation. Canonical records,
internal summaries and existing seed contents remain unchanged. Selected
inspection retains complete model/effort choices; account, launch and OpenCode
catalog responses retain their separate arrays. No recursive/null/prefix filter
is used. Official current and last Stable readers get session model choices from
verified selected details. Out-of-tree raw list JSON consumers are not audited.

The extended fixed-public-field accounting uses only names compiled into the
helper, never keys discovered from its input. Per current/archive group it reports
property present counts, encoded value sizes and independent encoded removal
deltas. This is size accounting, not permission to omit any other property.
Frozen V1-V4 five-key helper bundles and their measurements remain historical
evidence; a six-key run must use its separately sealed source/library.

The current and v0.48.3 selected-choice source audit is:

| Reader | Model choice source | Delivery check |
| --- | --- | --- |
| Desktop frontend | `store/session.ts` supplies selected followed Activity; `session-model-settings.ts` reads only `details.model_options` | The model store keeps the exact selected run/conversation and refreshes details |
| Android | `SessionDetailsUi` reads `SessionDetailsPresentation.settings(model.activity)` | `ZerusViewModel` requires matching selected target and verified Activity before evidence and rechecks it before send |
| iOS | `SessionSheets` and `SessionHeader` read settings from `model.activity` | `ConversationModel+Actions` requires matching verified selected Activity and rechecks fresh evidence |
| Rust and retained Python connector | Fresh native inspection, not a cached list array | Exact name/run/conversation, support, pending/native choice checks remain unchanged |

The array is still produced by shared `effort::summary` and provider full-detail
inspectors. A public-list projection must never be moved into those functions.
