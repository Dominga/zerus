# Proposed bounded pre-anchor evidence

Status: pre-code design for `zerus-5wmh.66.2.34.3.5.2`, following the
[shared CLI-origin requirements](desktop-quit-cli-origin.md). This slice can
collect refusal evidence. Its sole production availability result is
**Unavailable**. It has no production read-root or anchor constructor, helper,
connection, installer, pairing build or delivery caller. No positive native
origin or initial-admission proof is claimed.

The initial ordering gap remains: the GUI must reserve its original summary,
generation and physical resources before helper preparation/spawn, while proof
required before its first production probe cannot depend on that helper. This
proposal introduces no bootstrap exception. A descriptor, digest, input manifest,
passive G1 observation or decoded child record cannot resolve the gap by itself.

## Proposed native boundaries

The future implementation would keep these private, separate types:

- `NativeEntryInputs`: raw OS arguments and relevant environment observations
  captured once at the first native entry, before ordinary argument conversion,
  `Config::load`, `--hgs` conversion, threads or selector I/O. Its constructor
  takes no caller data. It has no serde constructor, raw-value Debug or public
  fields. This records what native entry observed; it does not establish absence
  of earlier loader effects or GUI-origin authority.
- `RefusalEvidence`: closed reason codes, bounded counts and optional digests.
  This is diagnostic data, never a permit. It may report an observed mismatch or
  an unqualified prerequisite, but no positive variant or conversion to
  `ProductionAnchor`, G1 role/ticket or a resource lease exists in this slice.
- Private image/configuration read observations: bounded metadata and captured
  bytes, separate from authority. For initial mechanism qualification they are
  produced only inside a typed fixture root. That root cannot be promoted to a
  production root by a path, environment flag, JSON value or matching digest.

Native raw-input capture does not mutate or normalize the environment. Record
presence and original OS bytes, including empty and non-UTF8 inputs; never use
lossy string conversion to decide absence. Proposed capture limits are 256
relevant keys, 512 arguments and 64 KiB aggregate raw bytes. Over-limit evidence
refuses without truncating an input into a permissive value. Keep the original
capture after known loader writes; a stripped or late snapshot cannot replace it.
These limits bound this diagnostic acquisition, not the separate 16 MiB quit
registration or 50,000-target action contract.
Apply bounds before copying/allocation, including API/iterator construction:
stopping an iterator after a limit is insufficient if its constructor already
materialized the whole environment. The exact native OS capture backend and its
pre-thread lifetime/safety proof need separate agreement before code; fixture
raw inputs are data and cannot construct a native original-scope witness.

The production evaluator initially performs only memory inspection of that
native capture and already embedded constants, then returns Unavailable. It
does not inspect the owner's executable/configuration or look for an anchor.
Image/configuration mechanisms are fixture-only until a concrete native
read-root acquisition contract receives its own exact review and proof. This
document does not supply that missing constructor.

## Acquisition and authority matrix

Every row distinguishes evidence from permission. An unknown fact remains
unknown; a skipped or refused observation cannot become a negative-use proof.

| Required fact | Bounded acquisition proposed | What it establishes | Missing/unknown result |
| --- | --- | --- | --- |
| Original native-entry input | One raw in-memory capture, within the limits above | Exact values observed at that entry | Unavailable; no later recapture or override stripping |
| Explicit selectors/markers | Raw prefix/key presence and exact original arguments before parsing | Refusal evidence under the current strict selector inventory | Any presence forbidden by that inventory, unclassified prefix or over-limit input refuses before I/O |
| Loader environment and earlier effects | Complete source-derived inventory and separate native provenance proof | Nothing from main-ELF identity or an entry-time capture alone | Unavailable; no assumption that constructors or inherited loader controls were harmless |
| Native OS user/default home | Exact native UID/profile provenance, with an audited bounded call graph | A source for default derivation only after qualification | No fallback to HOME or a caller record; an unqualified lookup is not attempted |
| Running GUI/build custody | Platform's shared G1 contract and independently qualified running-image/build evidence | Identity observations, not a quit role or anchor | Unavailable; a pathname, self-reported build JSON or copied provenance file is not a substitute |
| Embedded Linux CLI expectation | Trusted build's actual artifact digest/format descriptor in the native GUI | Which artifact bytes are expected | Missing/unpaired expectation refuses before candidate reads; no runtime digest input |
| Selected CLI file | Fixture-root fd-relative no-follow regular-file acquisition and before/after native metadata | Which file was read at that observation | Unknown ancestry, replacement, special type or unsupported source refuses; no production discovery in this slice |
| CLI bytes | At most 256 MiB, read in 64 KiB chunks through the same pinned fd | A bounded byte observation matching or differing from the expectation | Read error, cap or generation change refuses; equality is not loaded-page proof |
| Linux loaded pages | Later original charged exec plus actual kernel/filesystem write-exclusion and image/birth proof | Nothing before that later qualified stage | Unavailable; no pre-anchor child, generic ETXTBSY assumption or post-exec hash shortcut |
| Mac pair | Existing G1 own sealed signed pair, `.25.2` and genuine `.25.1.2`, with current origin/custody checks | Existing passive platform identity facts only | Unavailable until every gate qualifies; no alternative Mac selector or bare CLI fallback |
| Configuration inputs | Fixture-root pinned descriptor reads, 64 files/4 MiB total/128 KiB per file | Immutable captured bytes and metadata, not loaded configuration authority | Partial, oversized, unreadable, replaced or incomplete input set refuses |
| Native derivation/fleet | Separately agreed pure bounded derivation from the complete captured manifest | Only the exact configuration/routing actually derived, once qualified | Unavailable; do not execute hgs/ssh, call Config::load or trust fleet JSON to discover it |
| Indirect configuration/endpoint provenance | Complete source-derived input and actual-endpoint contract | No current proof from an alias or absent field | Unavailable; no SSH configuration evaluation, credential helper or network probe in acquisition |
| Child received scope | Later charged child's pre-loader capture compared with the parent's sealed derivation | Consistency with what its parent supplied | Never independent proof of the GUI's original environment |
| Initial production namespace | No acquisition operation in this slice | No authority | Zero attempted production parent/bootstrap/anchor stat/open/create/read/write/lock operations |

The source-literal inventory at the prior origin pin found 75 prefixed keys in
598 matches across 71 Rust files. It includes tests, writes and comments; it is
an over-approximation, **not** a complete read/call-graph audit. Implementation
review must trace wrapper/prefix reads and loader controls as well as literals.
The existing HGS/CLIENT selector inventory cannot silently stand in for a complete
dynamic-loader or indirect-configuration inventory.

OS account lookup, Security APIs and filesystem metadata can have indirect or
blocking behavior. Do not label a call harmless because its public API looks
read-only or synchronous. The precise call graph, side effects, bounds and
executor lifetime need platform agreement. An unqualified call is not attempted
by this production evaluator. In particular, do not presume an OS-account lookup
has no NSS/IPC behavior, or borrow G1's passive selector as a new quit executor.

## Mechanism-only fixture read set

The initial private fixture mechanism may read only its supplied typed root and
selected regular leaves below it. For Linux, propose this exact acquisition
sequence, subject to native review and private kernel qualification:

1. Pin the fixture root and traverse at most 64 fixed relative components and
   4096 path bytes with `O_PATH | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC`. Reject
   absolute paths, dot/dot-dot, links, unexpected ancestry and unqualified
   filesystems. No directory scan, include discovery or PATH search fills a
   missing input.
2. Acquire the leaf with `O_PATH | O_NOFOLLOW | O_CLOEXEC`, then inspect that
   descriptor with `fstat`/`fstatfs`. Refuse non-regular files, unknown owner/mode,
   unsupported filesystem and oversized input before any data-open attempt. A
   check followed by an ordinary pathname open and later `fstat` is insufficient:
   replacement by a device could already have caused effects at open time.
3. Only for that pinned regular object, obtain a read descriptor through its
   exact entry in a pinned, verified kernel procfs view of this process's fd
   directory. This is a controlled kernel-fd reopen, not permission to follow
   caller links or scan another process. Keep the original `OwnedFd` private and
   alive; its number is generated internally, never supplied by JSON or reused
   after close. Reopen read-only with `O_CLOEXEC | O_NONBLOCK | O_NOCTTY`, then
   verify the read descriptor's type/device/inode/owner/mode against the pinned
   object. Missing, unverified or inaccessible procfs refuses with no pathname
   fallback. Procfs authenticity, namespace behavior, fd lifetime/reuse and
   permissions are explicit qualification obligations, not supplied by a string
   spelling such as `/proc/self/fd`.
4. Read only through the verified descriptor within the image/configuration
   bounds, then recheck source/ancestor and file observations. Changed or
   incomplete observations refuse. `O_NONBLOCK` is not a regular-file read
   deadline; no timeout or future executor-lifetime guarantee is inferred.

Linux's [open manual](https://man7.org/linux/man-pages/man2/open.2.html)
documents `O_PATH` as a descriptor reference without opening the object for file
I/O; a no-follow symlink result must still be rejected by type. The
[proc fd manual](https://man7.org/linux/man-pages/man5/proc_pid_fd.5.html)
documents fd links and their access checks. The bounded regular-file reopening
sequence above is a proposed use requiring the listed native proofs, not a claim
that these manuals establish fixture or production authority.

Darwin has no admitted backend in this slice: do not emulate these Linux flags
with a path check followed by a potentially dangerous open. Reuse of G1's Mac
contract remains separate and unavailable until its own gates qualify. All
metadata/read/hash observations record exact before/after identities; they
cannot certify write exclusion or replace a mapped-image proof.

Read-side effects, including atime changes or lease notifications, must be
identified in fixture qualification; read-only flags are not a universal claim
of no kernel effects. This supplies no production read-root or permission to
notify an existing process. Unknown effects remain unavailable.

Keep raw captured bytes private and bounded, without logging configuration,
session names, targets, paths or environment values. Only closed refusal reasons,
counts and permitted digests leave the evaluator. No retained record, journal,
spool or new shared lock is written. The fixture's synthetic report is clearly
separate from production diagnostics and contains only reserved fixture data.
Proposed acquisition caps are 8 MiB of owned buffers/metadata and 136 owned fds
at once, including root/ancestor/procfs references. Apply limits before the next
allocation/open, close owned descriptors on every completed error path, and
release each copied configuration input's read handles when no longer needed.
A limit refuses; it never truncates the manifest, starts a helper or extends the
original Quit cutoff. These are proposed fixture-acquisition caps, not a new
global quit-resource admission or permission to allocate before its transaction.

Do not start a thread, child, connection or status command to finish acquisition.
Do not invoke shell, hgs, ssh, lsof, codesign, an installer or a service. This
evaluator does not gain an unaccounted worker by calling it a passive check.
Any future production metadata work needing a managed executor must return with
its exact lifetime/accounting contract for review; absence of such an executor
means unavailable, not work queued past GUI exit.

## Original ordering and later charged execution

| Boundary | Permitted result in this slice | Forbidden inference |
| --- | --- | --- |
| Native entry before loader mutation | Capture raw inputs; record closed refusal evidence | Entry-time capture proves earlier loader behavior or native authority |
| Group registration availability | Fresh evaluation within this read set; Unavailable | Probe a child/connection/anchor to decide whether a toast note is needed |
| Real Quit | Revalidate original context and refuse without production probes | Reuse the registration note, cached Boolean or an earlier permissive snapshot |
| Initial summary/generation/resource transaction | Not entered by this evaluator | An image hash, captured config or static G1 selection authorizes the first open |
| Original helper preparation/spawn | Only after a separately admitted original transaction; absent here | Start an origin-only/version-check child before its reservation |
| Charged descriptor execution | Future native implementation binds the exact pinned source and captured inputs | Re-resolve a path/configuration or label child-received data GUI-original proof |
| Later image/park/handshake checks | Future actual child birth, loader/image and consistency proof with resource custody | A matching main-ELF hash or a promise to park proves zero earlier production probes |
| Failure after a real charge/partial hold | Existing denial/manual recovery and unresolved resource retention | Delete evidence, refund unknown custody or fall back as though nothing was reserved |

If a required fact is available only after spawn, connection or anchor access,
that row cannot authorize initial admission. No proof is borrowed from a G1
operation ticket, TaskStart ledger, another state root, old approval or decoded
phase. The later child/exec/drain mapping must be reviewed against the original
transaction and original five-second cutoff; no new acquisition/helper deadline
renews a request. No new positive variant is added by this matrix.

## Refusal presentation and verification boundary

When the qualified caller eventually lands, group registration's fresh native
availability evaluation still obeys this read set. With the current unresolved
initial proof it returns unavailable without probing and may select only the
agreed fixed translated core Text sentence in the existing Undo toast note for
the whole original group. A failure first discovered at Quit is diagnostic only.
Current UI/logging remains unchanged until that caller lands. No popup, restart
prompt, confirmation, exit delay, next-start notice or new skew surface is added.

Qualification of a future implementation must log raw hashes and actual counts:

- Native raw-input capture at the real first entry, before each known loader
  write, with present/empty/non-UTF8/unknown/over-limit values and changed later
  context. A late or stripped capture never replaces the original.
- Fixture source/path/ancestor replacement, symlink/special-file refusal,
  boundary sizes, short/error reads and concurrent changes. Byte equality after
  a modify-then-restore writer remains evidence only, never loaded-page authority.
  Record zero attempted data opens of a substituted special object, not merely
  refusal after opening it; qualify the Linux pinned-fd/procfs sequence and
  absence of a generic-path fallback separately.
- A decoy production view plus an observer that refuses and counts every
  attempted production parent/bootstrap/anchor probe. Record attempted counts,
  not merely successful opens, and unchanged decoy identities. Never inspect the
  owner's actual OS-profile anchor, selected CLI or configuration in these tests.
- Zero evaluator-created children, threads, connections, processes, commands,
  mutation calls and shared locks, including on errors/timeouts. Distinguish a private
  harness process from operations initiated by the evaluator; report both counts.
- Child-scope equality and mismatch as consistency data only. No native permit
  from JSON, a fixture root, matching bytes or an observation with a positive
  Boolean. No current production caller is introduced by the test harness.

This is a pre-code matrix, not those executed controls. G1/G3/native agreement
on the exact in-process acquisition contract precedes implementation; loaded
image/source immutability, complete configuration derivation and initial positive
authority remain future separately pinned gates. Mac genuine paired origin,
strict selector refusal, R3 legacy-mutator exclusion, store/VFS, all actual
resource drain, exact release and same-or-earlier agreed-and-shipped release UI
remain independent. Windows stays fenced.
