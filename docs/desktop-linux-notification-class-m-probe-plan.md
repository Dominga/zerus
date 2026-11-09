# Private Class M probe: pre-instrumentation plan

Status: revised proposed design, 2026-11-01, G4 `.14.5.10`. No allocator implementation,
probe execution, SDK execution or measurement is part of this change. This plan
implements no permission. It needs fresh G1 and G3 agreement before allocator
code, followed by exact implementation reviews before a separately scheduled
private execution campaign and raw-proof review. The source/measurement contract
in [Class M](desktop-linux-notification-class-m.md) remains authoritative.

The source baseline is PR #844 merge
`4fe1e86752f0a5d9b4a6f17409ad37d8c542aa22`. All current-source, negative-consumer,
production, physical-memory and whole-profile gates remain open. A usable G1
export takes priority if G1 announces one. The topology remains one original
startup/worker/connection, two logical raw16 readers and sequential control calls.
The probe never releases B: only the original graph destruction, exact CleanExit
and actual original join can release the existing fixed group.

## Target, toolchain and absence proof

Use a dedicated **lib-test build variant**, rather than a new public Rust API or
an integration target importing a copy of startup. The existing startup tests
can already reach the private constructor and exact original Completion. The
proposed module is `startup/tests/class_m_probe.rs`, reached only through
`all(test, target_os = "linux", target_arch = "x86_64", g4_class_m_probe)`.
The global allocator definition is private to that test variant. A separate
mutually exclusive `g4_class_m_control` variant delegates to System but never
tracks or arms, from its first callback to process exit. No runtime environment
variable switches an unmeasured process into measurement.

This choice requires G1/G3 agreement that the separate artifact is the dedicated
test target for this contract. It adds no Cargo feature or manifest target, no
startup wrapper/export, no copied SDK or transport, and no production caller.
The later build script only declares the two custom cfg names for check-cfg.
Invalid combinations (both cfgs, non-test, non-Linux, non-x86_64 or no `live`)
produce a compile error in this crate. Instrumentation source is never selected
in an ordinary test or production build, including an accidental private cfg in
a production build. The normal default and media feature closures are unchanged.

The private builder starts with an environment built from scratch and separate
HOME/HGS/XDG/TMPDIR/TMUX_TMPDIR, private source/registry caches and target directory.
It runs the existing package's locked `cargo +1.99.1 test --lib --features media
--no-run --message-format=json`, with only the reviewed custom cfg in
CARGO_ENCODED_RUSTFLAGS. No inherited RUSTFLAGS, loader injection, allocator
tunables, owner bus/display or account configuration is permitted. Dependencies
receive the same cfg but no dependency source changes; their exact feature and
compiler closure must be recorded. Copy the one reported test ELF into the sealed
campaign directory, name it `g4-class-m-probe`, hash it and invoke only the exact
private `class_m_probe_child` test with `--exact --test-threads=1`.

Require four source-matched artifacts before executing a probe: normal default
lib-test, normal media lib-test, normal production app and private media probe.
Use unstripped symbol tables and link maps, not strings alone. The private ELF
must contain `G4_CLASS_M_PROBE_STORAGE_V1`, the allocator implementation and
`zerus_g4_class_m_probe_identity_v1`; all three normal artifacts must contain none
of these, no probe TLS marker and no probe-sized section. Verify object/archive
members too, and record `.bss`/TLS sizes. Absence in a stripped binary is
insufficient. The never-armed control has its own explicit marker. No normal
parallel test campaign may execute the private artifact.

Pin the complete source/lock/features, Cargo/rustc/linker paths and binary hashes,
ELF interpreter, all resolved DSOs/build IDs and relevant System sources before
the implementation/execution reviews. The discovered candidate is:

| Item | Candidate pin; revalidate before implementation/execution |
| --- | --- |
| Rust | 1.99.1, commit `58cf8f9c35b355199727627d07ae34a4e2846d67`, LLVM 23.1.1, x86_64-unknown-linux-gnu |
| rustc binary | SHA-256 `b53b780f77f952062f8ca8a3b70aef2390b1e45ad91939be94afdc9543026853` |
| libc package | `glibc 2.44+r50+g1848099f063e-1` |
| libc binary | SHA-256 `02c8c3d06180beee9c7d0392aa85736c7d19f274a81e7f8f0b3d9e28e0641524`, build ID `d320029ad1a23be2f4bce117fd321cbd23a2014b` |
| Kernel discovery | 7.2.11-arch1-1, x86_64; record actual execution kernel separately |

These are discovery facts, not an executed probe closure. The packet exports
the exact Rust commit's `std/{alloc.rs,sys/alloc/{mod,unix}.rs}`, native TLS macro
and core GlobalAlloc contract, plus glibc commit `1848099f063e` malloc/arena/version
sources. Installed rust-src is absent; the exports come from the corresponding
upstream commit, not an assumed local source tree. System remains System:
ordinary layouts use malloc/calloc/realloc/free; high alignment uses the existing
posix_memalign and System fallback. The wrapper does not replace fallback copying
or zeroing with its own implementation. No libc physical behavior is measured
by reading these sources.

## Static layout and initialization

The candidate has 262,144 slots and 262,144 bucket heads, statically zeroed from
process load. A slot is repr(C), 48 bytes on this target: address, requested size,
alignment and generation (four u64), epoch mask and chain/free link (two u32),
state byte plus seven reserved/padding bytes. States are Free, Live, Freeing and
Resizing. Addresses are numeric identity keys only: never dereference them,
reconstruct a pointer from them or print them. Generation zero and slot index
zero are sentinels; indexes are encoded as slot+1. Generation wrap is unavailable.

| Fixed storage | Planned bytes |
| --- | ---: |
| 262,144 slots at 48 | 12,582,912 |
| 262,144 u32 bucket heads | 1,048,576 |
| Header, atomics, counters and epoch metadata | 4,096 |
| 128 coherent boundary records at 512 bytes | 65,536 |
| Report buffer | 65,536 |
| Probe-control holder, 262,144 provenance-preserving pointers | 2,097,152 |
| Total planned static storage | 15,863,808 |

All probe statics, alignment, padding, control holders and reserved regions must
fit in **16,777,216 bytes total**. Compile-time size/alignment assertions and ELF
symbol/section accounting must verify the actual layout; the table is a plan,
not a compiled footprint claim. No extra static side table is allowed. The
one-byte per-thread recursion flag has const native TLS initialization and no
Drop; actual TLS section/loaded-thread costs are separately reported and remain
outside any physical-memory qualification. Stack tokens and hook costs are
identified too. No heap storage initializes the probe.

The control holder is fixed MaybeUninit storage of actual `*mut u8` values,
eight bytes each on this target, with an initialized-prefix count. Store the
original returned pointer with its provenance; never store an integer and later
reconstruct a pointer for cleanup. Its exhaustion case uses one fixed, valid
Layout for every entry. Other controls retain the original typed pointer and
Layout on their stack or through an explicitly owned thread transfer. Clean up
only initialized, still-owned entries with those exact arguments. The identity
table's numeric address keys are never a source of deallocation pointers. Unsafe
holder access, initialized-prefix bounds and cross-thread ownership require the
later code review; this design does not approve their implementation.

The header contains an acquire/release atomic spin gate, a sticky atomic fault
bitset, checked u64 counters, next generation, free-list cursor and 32-epoch
state. Slot/bucket data use UnsafeCell only behind the gate. Do not form a
mutable reference covering atomics accessed concurrently; split disjoint fields.
Initialize free slots lazily by increasing a bounded cursor; use their link for
the recycled free list. No callback needs an initializing allocation or scan to
build that list. Hash the numeric address with a fixed integer mixer and mask;
collision chains are index-bounded, not external allocations. Corrupt indexes,
duplicate Live identities or a chain exceeding the slot count are unavailable.

The 4,096-byte header has fixed regions: 64 bytes gate/fault atomics; 64 bytes
epoch, generation, free-list and initialized-prefix metadata; 256 bytes for 32
global u64 counters; 1,920 bytes for 15 epoch summaries of 16 u64; 512 bytes for
64 event counters; 32 bytes record-validity bitmaps; 256 bytes control counters;
320 bytes for five 64-byte attempt descriptors; 672 bytes reserved/padding.
All padding counts. Compile-time assertions must reject any region overrun.

The 32 global counters are: for alloc, zeroed and realloc, attempted count/bytes,
successful count/bytes and failed count (15); dealloc entry count/bytes and
completion count/bytes (4); alloc pending count/bytes (2); realloc pending
count/new bytes (2); Freeing count/old bytes and Resizing count/old bytes (4);
non-Free count/bytes and committed-byte peak (3); transaction sequence and
callback entry count (2). Fault bits are separate atomics. Each epoch summary
holds birth count/bytes, resize attempted count/new bytes, resize successful
count/new bytes, resize failed count, dealloc entry count/bytes, dealloc completed
count/bytes, retained Live count/bytes, retained in-flight count/bytes and snapshot
count (16). Birth is assigned to the successful return epoch; attempts and
completions use their own entry/return epochs. Retained epoch membership overlaps
by design. No difference between those counters is relabeled as physical growth.

## Callback ordering, reuse and realloc

**Never hold the bookkeeping gate across a System call, formatting, I/O, a
fixture rendezvous or user code.** Only bounded integer/table operations run
under it. This avoids a probe-lock/System-lock ordering dependency. A callback
sets its const TLS recursion flag before entering bookkeeping; nested entry
sets the sticky fault and delegates directly to System, without touching the
table or waiting on its own gate. The outer callback observes that fault and
cannot publish a valid result. Clear only the TLS flag on known ordinary return,
never the global fault. No Drop that can panic, lazy TLS, Mutex/Once, logging,
heap formatting, OS I/O or unwinding is allowed in a callback. Exact generated
TLS/allocator code still needs implementation review; source intent is not proof.
Gate acquisition has a fixed ceiling of 1,048,576 compare-exchange/spin attempts;
on entry exhaustion set the atomic fault and call System once without accessing
the table; on return exhaustion preserve the already returned result without
another System call. No timeout clock or syscall runs in the callback. Preemption can therefore
make a measurement unavailable; it must never make a real System call wait
indefinitely for probe bookkeeping. A holder must still release its gate after
observing a fault. Exact instructions and bounded chain traversal need review.

Each callback has an entry transaction, one unchanged corresponding System call,
and a return transaction. The entry stores a stack token with slot+generation,
request layout and entry epoch where relevant. The System pointer/layout stay
unchanged in that stack frame. All gate operations recheck the sticky fault.
On any fault, preserve ordinary System behavior and treat all metrics as
unavailable. **Before System has run**, forward the original corresponding
method once, with the original pointer/Layout/new size. **After it has run**,
return that exact pointer/null/void result with zero additional System calls.
Apply this to alloc, alloc_zeroed, dealloc and realloc, including a genuine
failed realloc. Never repair history, suppress/free twice, change a
layout, free old storage on failed realloc, or fabricate a null/success result.
Fault injection changes bookkeeping only, never supplies an invalid System
pointer or layout. Callback entry is not signal-safe; no fixture invokes the
allocator from a signal handler or forks a measured child.

| Operation | Entry transaction | Return transaction |
| --- | --- | --- |
| alloc/alloc_zeroed | Record attempted call/size and outstanding request; capture epoch. | On nonnull create a new Live generation with exact size/alignment; on null record failure without a live identity. Remove outstanding request either way. |
| dealloc | Find exactly one Live address with exact size/alignment; mark that same slot Freeing and retain its identity/size in the live total. | Remove **the token's slot+generation**, not whichever record now has that address; return it to the free list after System dealloc returns. |
| realloc | Find exact Live address/layout; mark its slot Resizing, retain old identity/size and record outstanding new-size request. | Null restores the same slot Live with its old size/address/ownership after checking that no other Live record conflicts. Nonnull checks the same invariant, then relinks that slot to the returned address, changes its requested size and preserves its generation. Remove the outstanding request. |

An address chain may contain multiple Freeing/Resizing generations and at most
one Live generation. This is intentional: System can free an old address and a
different thread can receive/release/reuse it before the first callback returns.
A newly returned alloc address conflicts with a Live record, but does not
conflict with an older in-flight generation. Dealloc/realloc entry selects only
Live; return selects only its retained token. Never overwrite an in-flight slot
by address, recycle it early or use a single address->record map that loses ABA.
Successful realloc must find no other Live record at its returned address before
relinking. It may coexist with older in-flight generations at that address.
Calling the corresponding System::realloc once does not imply one physical
malloc/free internally; its unchanged high-alignment fallback remains System's.

All updates and coherent scalar snapshots linearize under the gate. A callback
can be pending across a snapshot; its exact outstanding request and Freeing or
Resizing identity remain explicit. There is no temporary disappearance of an
old allocation before a failed realloc. Multiple completed physical uses of an
address can overlap **in the interface accounting** while earlier calls remain
pending; these generations are retained separately, not mistaken for duplicate
physical ownership. The finite slot limit includes these in-flight records.

There are no data references or iterators into a table across System calls or
gate release. Token mismatch, invalid state or unexpected duplicate is sticky
unavailable. Bounds checks are explicit and checked arithmetic never panics.
No unsafe operation accesses allocation contents except unchanged System calls
with the caller's original valid arguments. Prove these invariants against the
actual code before execution; this document grants no unsafe-code approval.

## Metric and epoch semantics

Report intercepted API calls, not allocations the optimizer might have removed.
For each alloc/zeroed/realloc: attempted count and sum of requested result sizes
at entry; successful count and sum of requested result sizes at nonnull return;
failed count on null. These cumulative sums are traffic, not retained growth.
Report dealloc count/size at entry and completion separately. Checked counters
and all outstanding request counts/bytes are included; any overflow invalidates
the complete observation. Zero-size Layout calls are not generated: they violate
GlobalAlloc's contract. Zeroed allocation means alloc_zeroed, not zero-size.

The primary live metric is **requested bytes committed at callback accounting
boundaries**: sum of Live, Freeing and Resizing slot sizes. Add a successful
allocation on its return transaction, remove a free on its return transaction,
and replace old size with new size on successful realloc's return transaction.
Failed realloc keeps the old size. Record its peak after every transaction.
Separately report the outstanding alloc/realloc requested sizes and pending
old/free sizes, so an in-flight boundary is never described as quiescent.

This metric can retain an old requested size after System physically freed it
but before its callback returns. It cannot see new System storage before the
return transaction, System realloc's internal old/new overlap, usable size,
pages or any physical peak. It is an explicitly defined interface metric, not
instantaneous physical ownership or full SDK heap. No B/24F/baseline/snapshot or
unknown subtraction produces a Class M number.

Use a fixed 32-bit non-forgetting epoch mask per identity. Birth and each
successful resize OR both captured entry and return epochs. Free entry also ORs
the current epoch before its eventual removal. A boundary records for each epoch
the current size/count of every retained generation bearing that bit, including
pending states; sums across epoch columns are not disjoint totals. Birth/latest
resize alone never erases earlier membership. Failed realloc is attempted
traffic in its entry/return epochs but does not change retained identity size.
Epoch exhaustion is unavailable, never mask reuse or table reset.
An epoch transition also ORs its new bit into every existing non-Free record
under the gate, including Freeing/Resizing records. Thus buffers predating a
startup remain attributed to each boundary through which they survive; changing
an epoch cannot forget baseline retention. This bounded full-table walk is
observer work and stays included, never a callback or System call.

Epochs: 0 pre-test/harness; 1 reporting preparation/baseline; 2 cold startup or
warm preparation; 3 its joined boundary; 4..11 four warm repetitions with a
startup and joined epoch each; 12 after the final join; 13 report construction;
14 post-report. Decoder/probe-only children use epochs 2 and 3 for their one
case, and otherwise the same reporting epochs. Fine lifecycle phases are scalar
boundary IDs inside an epoch, not unbounded new epochs. Warm preparation and all
four repetitions stay tracked from process birth. Any unknown attempt stays
spent/pinned and terminates that child's repetition sequence without a retry.

## Hooks and coherent reporting

Use existing private startup Hook sites for BeforeNative, Connected, each
Matched, Raw, Message, DropReaders and DropConnection. Additional test-only
scalar sites may bracket (not move) ingress default decode/error normalization,
each sequential removal reply, original close return, executor/connection drop
and exact original join. They record a fixed phase ID/count and snapshot totals
under the same gate, with no Message/Connection/Arc clone, body consumer, new
control call, allocation or extra executor task. Do not patch dependencies to
observe internal allocations; global callback peaks include their diagnostics
even when only external before/after phases can be named.

Every callback updates the checked event/call counts and peak; a Message event
does **not** automatically consume a snapshot record. Revision 2 replaces the
64x1,024 layout with **128x512 bytes**, preserving the same 65,536-byte region
and the same planned total static footprint. Each record contains 16 u64 scalar
words (sequence, packed phase/attempt, fault bits, event count, non-Free bytes,
peak, pending alloc/realloc count/bytes, Freeing/Resizing count/bytes, event bitmap
and callback count), 15 epoch pairs (retained non-Free count/bytes, 240 bytes),
and 144 reserved/padding bytes. Per-epoch totals include all non-Free states;
state totals are global, not a claimed epoch-by-state cross product. Epoch IDs
0..14 are the only allowed IDs in this plan; a new ID needs a revised plan even
though the identity mask has 32 bits. A record is never overwritten or compacted.

The fixed slot schedule below is chosen **before launch**, including which
sites are expected or not applicable for each case. Unexpected duplicates,
overflow or a missing required site makes intended coverage unavailable. An
unreached branch stays explicitly unreached; do not invent a teardown record
after a failed attempt or reuse its slots for a later attempt. Source-site
sequence numbers retain actual order even when asynchronous events fill
different fixed slots out of numeric order.

| Slots | Predetermined use |
| --- | --- |
| 0, 1 | Pre-case baseline and completed report-resource preparation. |
| 2+24a through 25+24a, a=0..4 | Twenty-four slots for original attempt a, as specified below; five attempts only for warm preparation+four repetitions. |
| 26..29, pressure child only | Message delivery ordinals 1, 16, 32 and 64. The pressure child has only attempt 0, so these cannot overlap an attempt-1 record. All 64 deliveries and any unexpected extra delivery still increment checked counters. |
| 2+6i through 7+6i, direct children only | Invocation i=0 or 1: input ready; immediately before safe default receive/decode; before terminal scalar normalization; after normalization; receive returned/cancelled; exact fixture destruction. Known inapplicable sites are declared in the case inventory, never inferred to make a failed case pass. |
| 2..5, probe-only children | Before the named control, reached operation/trigger, after its known return, control cleanup. Prefix/entry/return fault reachability is separate scalar evidence, not a numeric value after a fault. |
| 122, 123, 124 | Sequence ended; initial report body prepared; coherent final prefix/footer snapshot. |
| 125..127 | Reserved, never used for extra traffic or a repeated attempt. |

The attempt-local slots 0..23 are: BeforeNative; Connected; AddMatch1 ack;
AddMatch2 ack; Raw; after **both actual readers** drop; before RemoveMatch1;
after its return; before RemoveMatch2; after its return; before original close;
after its return; before terminal receive-error normalization; after that
normalization; actual socket-reader/executor drain returned; before connection
drop; after connection drop; after executor drop; last original wake capture
gone; original worker exit; before original join; after original join; after the
original Completion operation; attempt outcome. The old DropReaders hook is
before drop and cannot stand in for the added after-drop scalar site; similarly
DropConnection is before close, not proof of actual connection destruction.
No hook relocates the original operation or creates a substitute join/completion.

The 64 event counters have fixed IDs: 0..23 for those lifecycle phase sites;
24 for the existing before-reader-drop hook; 25 for aggregate Matched; 26 for
every Message delivery; 27/28 for TickCreated/TickDropped; 29/30 for
BeforePark/AfterPark; 31 for ProbeSignal; 32/33 for before/after default decode.
IDs 34..63 remain reserved. Existing hooks alias the matching lifecycle IDs
where appropriate; counting their aliases does not allocate another record.
Every successful decode updates counters 32/33, without consuming a record in
the original-startup schedule; its terminal error normalization still fills
both required fixed normalization slots. Probe-only marker counters use their
separate fixed header region. New event IDs need a revised pin, not an overflow
sink or an unbounded vector.

A clean warm child has five separately numbered attempts, at most 120 attempt
records plus five common records = **125**. A pressure child uses 24 attempt
records, four predetermined message snapshots and five common records = **33**;
64 logical deliveries cannot exhaust the record region. A direct child has at
most two invocations, 12 invocation records plus five common = **17**. A probe
child has at most four control records plus five common = **9**. An additional
terminal normalization in one original attempt is unexpected and unavailable;
the locked SocketReader terminates on that receive error. Callback peaks and
all event counts continue for unsnapshotted traffic. No schedule adaptively
discards normalization, destruction, removal, close or join boundaries.

Encode a bounded binary report, rather than an unbounded textual table. Version
1 uses an at-most-8,192-byte header, up to 128 canonical 400-byte record payloads
(368 defined bytes plus 32 zero reserved bytes), and an at-most-2,048-byte
footer: **61,440 bytes maximum**, below 65,536. Encode integers explicitly in
little endian; do not serialize native structs/padding. The header carries the
fixed case/attempt schedule, validity/not-applicable bitmaps, global/per-epoch
counters and scalar outcome; no pointer or private address. The parent's bounded
decoder checks version, lengths, mask, sequence, counts and hashes, and retains
the original bytes. Decoded presentation is separate observer cost.

All participating threads, reactor/pool activity and cross-thread frees keep
tracking after join, during reporting and after output. Reporting preparation
allocations remain included. Construct the initial body from bounded storage
outside callbacks, write it without the bookkeeping gate, then acquire the gate
for slot 124. Copy counters, pending states, epoch aggregates and event counts
under that same gate; the final sequentially consistent load of sticky fault
bits defines prefix N while those counters are stable. Fault publication uses
the same sequentially consistent atomic order, including faults published
outside the gate on recursion/spin exhaustion. Include sequence N, those fault
bits and those exact pending counters in the final footer; release the gate
before writing it. Do not mix an earlier snapshot with a later unlocked status.
Slot 124 is transported in the footer, never as an earlier placeholder in the
initial body. The footer includes its complete canonical record, final validity
bitmap, sequence and initial-body byte count/hash. Earlier body records retain
their own sequences; the parent does not relabel them as simultaneous with N.

An observed fault through prefix N, missing/incomplete footer or process failure
invalidates numeric admission. The parent verifies delivery/exit, not an absence
of arbitrary future events. Callbacks and faults **after the final prefix check
are unmeasured**; they cannot be promised observable or used to claim process-exit
retention without another reviewed protocol. Tracking is not frozen/reset, no
owners are stopped and no group is refunded for the footer. Harness/libtest,
unclassified Rust allocations, infrastructure, report storage and observer costs
remain visible; their costs are never silently subtracted from an SDK total.

## Finite corpus and fixture scope

The following is the proposed inventory; exact generator bytes, scripts, scalar
hook source and all hashes must be frozen with the later implementation **before
any execution**. Changing an input or implementation means a new reviewed case,
not an unrecorded rerun. Missing coverage is unverified. No random search,
unbounded traffic, SDK fork or unsafe Message construction is permitted.

| IDs | Children and fixed recipe |
| --- | --- |
| P01..P04 | Four probe-only children: first-callback/pre-epoch history; alloc_zeroed plus alignments 8/16/64/4096 and sizes 1/64/4096; cross-thread free; bounded address reuse (256 successive 64-byte allocations). |
| P05..P07 | Three probe-only children: observed in-place shrink; observed moved grow; genuine System alloc/realloc failure under the fixed address-space ceiling. Old realloc contents/identity remain intact after null. A path not actually observed is unverified, never forced by changing System's return. |
| P08..P10 | Three probe-only children: free paused after System returns with old-address reuse; two successive reused generations still pending; realloc return raced with free/reuse and scalar snapshots. Fixed preallocated atomic rendezvous only, never under the table gate. Non-reached reuse is unverified. |
| P11..P16 | Six separate sticky-fault children: slot exhaustion; attempted-call counter overflow; generation wrap; return-token identity mismatch; entry-layout metadata mismatch; recursive callback entry. Each has exactly one first-fault trigger, detailed below. |
| P17 | Valid measurement control: non-forgetting cross-epoch grow/shrink/resize, cross-thread free and coherent report race. This must produce a valid tracked prefix and correct retained epoch memberships, not pass by becoming unavailable. |
| P18 | Early process death/missing footer; unavailable by expected missing delivery, never a numeric result. |
| P19..P28 | Ten independent forwarding-fault children: entry/return fault for alloc, zeroed, free, successful realloc and genuine failed realloc, one phase/operation/outcome per child. Each verifies one corresponding System delegation and no post-return replay, with original valid pointer/Layout/result. |
| D01..D08 | Eight direct-decoder children: F=65,536 in both endians, chunks 1/7/16/F, using the existing boundary-message recipe. |
| D09..D12 | Four direct-decoder children: declared H=4,096 and H+1 in each endian, with valid total F. H+1 must refuse before default body allocation. |
| D13..D20 | Eight direct-decoder children, both endians for each of four malformed fields: a wrong-type array of variants; a dictionary with repeated keys; a structure containing nested arrays; a nested variant/signature chain. Declared outer fields are short (16), nested data occupies remaining bytes up to F; keep required primary checks valid. Source-pinned depth/length limits select exact limit-edge and late-error recipes, not a claimed H-sized frontier. |
| D21..D24 | Four direct-decoder children: invalid UTF-8/name conversion; late truncation of a partially built nested tree; duplicate known-field replacement; malformed signature at its source-pinned depth limit. Fixed byte recipes are part of the implementation pin, not an exploit claim. |
| L01..L04 | Four original-startup children: cold clean; warm preparation plus exactly four clean repetitions; existing real private-bus raw16/full-F pressure; clean local-close EOF followed by actual original CleanExit/join. Distinguish queue fixture evidence from the worker's own two-reader lifecycle. |
| L05..L08 | Four direct-ingress children: unarmed empty EOF; partial-primary close EOF; partial-body close EOF; cancel after both flags/prior sticky refusal. Existing concrete SDK half and approved EOF predicate, no fixture inference of B release. |
| L09..L12 | Four original-startup children: failed setup; unanswered removal; original deadline expiry; lost public handles. One attempt each, unknown/pinned as applicable. A fixture kill is no clean join. |
| L13..L16 | Four existing actual-socket FD controls, one child each for auth, primary, body and outgoing; no FD negotiation or body consumer. |
| L17..L18 | Two auth children: wrong expected GUID before BEGIN/Hello; maximum cumulative A followed by over-cap refusal. |
| C01..C02 | Two separately compiled never-armed System-control children: cold clean and preparation plus four warm repetitions. No measured baseline is reconstructed from these processes. |

Revision 2 has **72 proposed child cases**: 28 probe-only, 24 decoder, 18
lifecycle/auth/FD and 2 unmeasured controls. These are not 72 numeric successes.
An original-startup child has one original attempt,
except the explicitly named warm children (one preparation plus four attempts,
each with an actual clean join). A direct-decoder/ingress child establishes only
its tested SDK path, never original worker completion. Decoder generators use
the existing explicit private Unix socket/locked SDK receive path; source-only
manual Message reconstruction cannot substitute for safe default decoding.
The external private bus/generator is a separately supervised process outside
the measured address space; any in-child input/preparation cost remains included.

P11 fills the fixed holder/table to exhaustion with valid single-layout
allocations. P12 sets only the selected attempted-call counter to u64::MAX just
before the named next entry increment. P13 sets only the next-generation value
to its wrap edge before the named allocation return. P14 corrupts only the
bookkeeping stack token's generation after System returns; the original pointer
and Layout remain intact. P15 changes only the expected slot's layout metadata,
so the next entry comparison faults while the actual System arguments stay
valid. P16 requests one bounded nested allocation before the outer transaction
acquires the gate; the const TLS recursion check is the first fault. The two
legitimate callbacks each still delegate their own System call once, and retain
their actual typed result for cleanup. No control supplies an invalid pointer or
Layout, rewrites a System result or clears the sticky fault.

P19/20 are alloc entry/return; P21/22 alloc_zeroed entry/return; P23/24 dealloc
entry/return; P25/26 successful realloc entry/return; P27/28 genuine failed
realloc entry/return. Each uses a single named test-only fault bit at that exact
phase. Its preallocated scalar markers identify trigger reached, System entry,
System return and observed callback return. A return fault must record zero
additional System entries after the saved result. A failed realloc must preserve
the original typed pointer, contents, Layout and cleanup ownership. Genuine null
or successful/moved/in-place results must actually occur; an unreached result is
unverified, never replaced by a synthetic allocator result or retried. The exact
targeting/hook/marker source must be pinned before code execution. These controls
prove their named forwarding branch, not another fault encountered afterwards.

Every child declares one outcome category: NUMERIC_VALID, CONTROL_UNMEASURED,
EXPECTED_FAULT_UNAVAILABLE, EXPECTED_LIFECYCLE_UNKNOWN or UNREACHED_UNVERIFIED; unexpected failure is
UNAVAILABLE. P01..P10 and P17 require numeric-valid probe coverage; P11..P16 and
P19..P28 require their exact first fault plus preserved System behavior; P18
requires the owned-death/missing-footer observation. A second fault after sticky
unavailable cannot qualify another condition. C01/C02 explicitly report
CONTROL_UNMEASURED, never numeric zero or a reconstructed baseline. Their
allocator is a never-armed System delegate with no identity table, probe TLS or
measurement counters; the candidate control-only footprint is a 16-byte marker,
4,096-byte outcome buffer and two u64 case/join counters (4,128 bytes), subject
to exact compiled ELF verification. It cannot be subtracted from the measured
child's footprint or infrastructure.

The intended numeric-valid rows are P01..P10, P17, D01..D24, L01..L07 and
L13..L18: 48 proposed cases, still conditional on complete reached-path evidence.
L08..L12 are five expected cancellation/unknown-lifecycle cases: preserve their
raw prefix evidence and exact unknown custody, but admit no numeric result or
completion from them. The 17 probe fault/death cases, these five lifecycle cases
and the two unmeasured controls complete the 72-case inventory. A valid prefix
in a direct decoder/FD/refusal case establishes only its named API observation;
it never grants original B release or a successful whole-startup qualification.

The nested-wire recipe is deterministic: zero-fill a frame of F bytes, set
endianness/type=signal/version=1/serial=1, declared fields length 16 and body
length F-32. At absolute offset 16 put field code 1, followed by the variant
signature at 17. Align each nested item by the locked D-Bus grammar; the generator
uses checked bounded writes and never reconstructs a Message unsafely. D13/14
use signature `av`, array byte length 65,508 at offset 24, then 16,377 variants
`[1, 'y', 0, 0]` from offset 28. D15/16 use `a{yv}`, array byte length 65,501 at
24, data at 32: 8,188 entries, each aligned to eight, key zero and variant byte
zero (five bytes per entry, three zero padding bytes between entries). D17/18
use `(avav)`, structure at 32: first array length 32,764 followed by 8,191 byte
variants; second array length 32,732 followed by 8,183 byte variants, ending at F.
These are wrong-type Path Values, so any late child allocation is tested before
the outer fields/conversion error; successful Message acceptance is not expected.
Actual reached decoder phases, not these source predictions, determine coverage.

D19/20 each send two fixed nested-variant chains, depths 63 and 65, terminating
in a byte variant, in their respective endian. D21 substitutes byte 0xff into
an otherwise valid one-byte object-path string; D22 uses D13's frame but leaves
the last nested variant signature incomplete while preserving the primary F
admission; D23 uses two otherwise valid member fields with values `A` then `B`
and the normal required signal fields. D24 sends signatures with 32 and 33
nested structures around `y`, then terminates the value at the signature. Record
actual depth/type/length rejection rather than assuming the outer context does
not consume a depth level. There are 27 direct-decoder invocations in 24 D
children (D19, D20 and D24 each have two); neither attempt is an original startup.
Existing full-F/H and authentication/FD recipes retain their reviewed source
generators, with complete current source exports in the implementation packet.

Original-close failure is a required **UNVERIFIED** control:
socket shutdown failure is not deterministically produced by an ordinary private
bus. No unspecified scalar fixture is approved. Do not add a replacement half,
SDK patch, synthetic CleanExit or cancellation waiver to force it. It remains
outside the 72 scheduled cases, explicitly unverified. A later separately
reviewed scalar fixture can prove only its named branch; real shutdown failure
stays unqualified unless actually reached. The same
rule applies if a nested recipe fails to reach its intended allocation/error
path: retain it with its reached boundary and label the intended coverage absent.

## Resource and evidence admission

The proposed campaign is serial, 72 children, at most 30 minutes total including
external fixtures and cleanup; each child gets wall 20 seconds, CPU 10 seconds,
AS 2 GiB, file size 8 MiB, FD 128 and core size 0. The proposed thread ceiling
means **eight concurrently existing tasks in the whole measured child**, not
eight ever-created threads or only threads observed by allocator callbacks.
It includes main/libtest, pre-epoch threads, workers, reactor/pool and transient
creation; external bus/generator processes are outside that measured group.
These are test ceilings, not product budgets. The full compiled
static probe footprint, DSOs, stacks/TLS and fixture/harness costs count against
the address-space setup. If that setup cannot start, record unavailable and
return for a reviewed limit change; never silently raise it or omit cases.

Thread enforcement is a separate **unadmitted execution prerequisite**. The
candidate method is a fresh private cgroup v2 with pids.max=8, under an already
delegated parent with the pids controller already enabled, and child placement
from its creation via clone3 CLONE_INTO_CGROUP plus a pidfd, before executing
the test ELF. No owner process moves; no shared-parent controller/limit changes;
no same-UID RLIMIT_NPROC, host-wide limit, posthoc sampling or unreviewed runtime
worker/interposer. Never-created-in-group and unverifiable delegated scope mean
execution unavailable, not a relaxed ceiling. The exact supervisor/source,
kernel/controller history checks, pre-exec placement, descendant refusal and
cleanup custody still require fresh G1/G3 agreement before scheduling; this
document supplies no such execution method approval. If this private admission
cannot be supplied, no campaign starts. Ever-created thread count is a distinct
history observation and is not inferred from the concurrent kernel limit.
The source basis for this candidate is the kernel's
[pids controller documentation](https://cdn.kernel.org/doc/html/latest/admin-guide/cgroup-v2.html#pid)
and the Linux man-pages [CLONE_INTO_CGROUP contract](https://man7.org/linux/man-pages/man2/clone.2.html).
Those API descriptions do not prove host delegation or the proposed runner.

Output is at most 64 KiB report plus 4 MiB complete raw log per child. The parent
enforces limits while draining output; exhaustion is unavailable, not truncated
green. Record byte counts/hash before interpreting. Missing report/footer, OOM,
timeout, unreached injection, signal, unknown callback history, recursion or any
probe fault invalidates numeric observations. Expected fault cases may prove
fail-closed behavior only, never a heap bound or successful original completion.

Every launch uses a child environment built from scratch with private HOME/HGS,
XDG/TMPDIR/TMUX_TMPDIR and private D-Bus plus Xvfb. The runner must pin its own
child PID/start/image/private environment and pidfd before any owned-child
termination; no owner/native-agent/tmux/service process is touched. Abnormal
scopes and raw evidence stay retained. No installed GUI launch or update occurs.
The later implementation review must pin the exact runner, child inventory,
resource setup and scalar hooks before scheduling execution.

Record source/lock/feature/compiler/linker/DSO/binary hashes, compiled static/TLS
footprint, all attempted/executed/unsupported/unavailable counts and every raw
log/report hash. Keep failures in the inventory. Fresh exact G1/G3/webui
implementation reviews precede execution; execution and raw-proof review are
separate from this design. Finite observations cannot establish a numeric SDK
envelope without the complete source/lifetime argument, or any physical/whole
profile bound. No metric grants currentness, effect settlement, replay, Notice,
Notify/CloseNotification, role handoff, factory, writer or production admission.
