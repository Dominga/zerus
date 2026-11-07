# Private Class M supervisor contract

This source-preparation contract complements the [Class M measurement
contract](desktop-linux-notification-class-m.md) and the proposed
[private probe plan](https://github.com/ufna/zerus/pull/847). G1 and G3 accepted
the exact bounded method and the locked SIGCHLD ABI clarification on 2026-11-01.
That agreement permits implementation preparation only. It grants no unsafe
implementation approval, native execution window or numeric memory envelope.
Source, ABI, ELF/DSO, control and raw-proof reviews remain separate gates.

The ordinary Xvfb profile and an eligible already-delegated parent are still
unavailable. Nothing here authorizes provisioning, an owner display, a guessed
profile or an expanded descendant graph.

Retain the probe plan's 72 proposed cases, 27 direct decoder invocations and
all classification, resource and exclusion rules. The fixture's trusted cooperating,
nonmigrating-source limitation is accepted by G1/G3: it is NOT hostile-sameUID,
transient-interference, unconditional historical eight-task, currentness or
whole-profile proof. No stronger isolation or production authority is added.
Observed drift always refuses. Empty subtree_control discovery supplies no
eligible parent and authorizes no controller activation or fallback.

Cgroup admission and private environment requirements remain mandatory: an explicitly supplied held cgroup-v2 domain parent of the
same namespace, with pids ALREADY enabled, owner/permissions and identities
rechecked; exactly one fresh empty nonce leaf; pids.max8 set/read back BEFORE
clone3 INTO_CGROUP; current/peak/events initial baselines and non-resetting
history at every admission/event/final boundary. Missing pids.peak, changed
ancestor limits/configuration, failed admission or drift is UNAVAILABLE. No
process is moved, no subgroup is created, and no parent controller is enabled.
Case envp is built from scratch with private HOME/HGS/XDG/TMPDIR/TMUX_TMPDIR and
only that case's display/bus, with explicit Unix fixture addresses and no owner
configuration, autolaunch, Qt seed, mobile path or implicit source lookup.

## Exact owners and original-parent death protocol

Normal components remain one single-threaded Python driver and one
single-threaded C launcher/tracer, plus separately inventoried bus, Xvfb,
generator and the selected target. There is no additional normal guardian.
The C process now owns creation/reaping of ALL its fixture children as well as
the target. External fixtures are created outside the target leaf. The driver
owns the C process pidfd; C owns each original clone3-created leader pidfd and
all tracee thread pidfds. Ownership is not reconstructed from PID/name/group
or transferred by a status message.

Before any child, each reaper explicitly REQUESTS SIGCHLD=SIG_DFL, empty mask,
sa_flags0 (no SA_NOCLDWAIT), queries it back, and remains the sole waiter/reaper.
For this exact pinned x86_64 glibc1848099f063e candidate only, the wrapper's
SET_SA_RESTORER adds0x04000000 before the kernel call, and the query copies the
kernel flags back. Literal queried-flags==0 would therefore reject that ABI
artifact. Agreed readback: require SIG_DFL; reject EVERY queried flag other
than this one exact ABI bit (allow0 or0x04000000); in particular SA_NOCLDWAIT,
SA_NOCLDSTOP, SA_SIGINFO and other handler/auto-reap flags remain forbidden.
Compare the complete actual C/ctypes flags value exactly with 0 or 0x04000000;
do not narrow, mask or truncate it before that comparison.
This does not request a handler, raw syscall substitute, restorer code or another
reaper. Do not call, dereference, print or treat sa_restorer as identity or
permission. The actual libc/loader/source closure still needs its exact pin.

Initialize requested AND query masks with checked sigemptyset before use.
Validate query-mask emptiness using checked sigismember for each valid signal
1..64, with the candidate _NSIG==65 separately asserted; every result must be0.
Do not compare opaque128-byte sigset_t object storage or its padding as a mask
predicate. A nonzero/error/member/unknown flag, changed ABI or failed query is
UNAVAILABLE before child creation; no reset, auto-reap or alternate wait path.
The source observation is not a measured readback or binary/source-equivalence
claim. H01's later pinned baseline must record the exact requested/observed
flags and semantic-mask checks as bounded scalar evidence; no native window or
additional child or control is granted by this contract.
The Python-to-libc sigaction declaration must use the pinned actual x86_64 ABI,
with implementation/compile assertions; no guessed ctypes layout. No signal
handler, background thread, Popen object/finalizer or other waiter may reap.
Unsupported/unverified state is UNAVAILABLE before creation.

The driver opens a pidfd for its own original live process, creates only the C
launcher via os.fork and fixed exec, and opens that child's leader pidfd before
any poll/wait/reap. This post-fork pin relies explicitly on default SIGCHLD,
no SA_NOCLDWAIT and the sole-reaper/no-reap interval, including an unreaped
early-exit zombie; it is not a generic Popen guarantee. Fork/exec failure records
its actual attempted/spawned/exited/unknown state. No C action is permitted
merely because fork returned a numeric PID.

The C main's first setup installs PR_SET_PDEATHSIG(SIGKILL), then verifies BOTH
getppid()==the driver PID supplied BEFORE fork and the inherited original-driver
pidfd is not terminated. The parent identity is never freshly inferred after
reparenting. Error/mismatch exits without leaf/fixture/target creation. Death
before the setter is detected by the subsequent original-parent checks; death
after arming delivers the process-directed signal. Both creating threads remain
single and alive for their process's whole role. All exec images are ordinary
non-setuid/non-setgid/no-file-capability files; no credential-changing operation
that could clear PDEATHSIG is permitted by the reachable source closure.

C emits GUARDED_READY and waits for REGISTER on its private command stream.
The driver sends REGISTER only AFTER binding/storing its C pidfd/start/image/
private scope. C may prepare fixtures/leaf only after this exact registration.
The driver closes its target-report write aliases before sending START_TARGET.

For every C clone3 child, C obtains CLONE_PIDFD atomically and retains that
original leader descriptor. The child receives C's preexisting self-pidfd and
pre-clone PID. Before any target/fixture exec it installs PDEATHSIG(SIGKILL),
checks getppid plus that original-parent pidfd, and refuses on any discrepancy.
It never computes a replacement parent identity. The same-credential exec keeps
this guard; source/ELF/capability assumptions are pinned, not presumed.

The target also has a private pre-exec SOCK_SEQPACKET bootstrap pair, distinct
from its later one-way target control pipe. Each end is owned by C or that one
child and CLOEXEC. Fixed64-byte ARMED/RELEASE frames bind case/nonce/sequence;
max8 frames/512 bytes total. EOF, truncation, partial send, unknown/wrong frame
or deadline is unavailable and cannot advance to exec. This is an explicit
separate setup channel, not a bidirectional use of FD14. Its actual source and
native implementation still require review.

Child ordering is: death guard/check; ARMED; PTRACE_TRACEME; original SIGSTOP;
then, only after continuation, consume RELEASE and recheck original-parent
life; remap/rlimits; one intended execveat. C ordering is: retain root clone
pidfd and original identity; receive ARMED/original stop (arrival order does not
alter requirements); bind pre-exec identity; install the full ptrace mask;
journal OPTIONS_ARMED; send RELEASE; continue. No target exec/progress occurs
before registration and option arming. EXITKILL is claimed ONLY after SETOPTIONS,
never for the earlier birth/setup interval.

If C dies before root identity publication, the driver does not adopt a numeric
PID from a file/group. Parent-death/EOF checks prevent authorized target progress,
but unobserved child exit/custody remains UNKNOWN. Stop that campaign, retain
the root/leaf/evidence and make no cleanup/admission claim from empty counts.
Scheduling delay or uninterruptible IO is no promised finite disappearance.
The same rule covers interrupted registration, missing ARMED/options/release
evidence and driver loss. The original owner keeps its descriptor until final
known reap; a lost owner is not silently replaced.

## Process leader, thread descriptor and wait states

Required actual ABI: clone3 INTO_CGROUP/PIDFD, pidfd_open PIDFD_THREAD and the
corresponding pinned kernel/headers (candidate Linux7.2.11). Unsupported flags,
permissions or wait semantics mean unavailable; no pidfd_open(nonleader,0)
or numeric-TID fallback. Initial root CLONE_PIDFD is a PROCESS/leader descriptor.
At its first stop also pin a PIDFD_THREAD descriptor for the leader's task.
Every later nonleader uses pidfd_open(tid,PIDFD_THREAD), not flags0.

Full inherited ptrace options are TRACECLONE/FORK/VFORK/EXEC/EXIT and EXITKILL.
Require source/DSO closure with no CLONE_UNTRACED, detach/SETOPTIONS changes,
unobserved task creation, cgroup migration or later exec. Events alone do not
prove that exclusion. Actual TGID distinguishes a thread from a new process;
unexpected process descendants refuse before their first continuation.

For new tasks require BOTH the creator event/GETEVENTMSG identity and the
automatic child's initial stop. Either may arrive first. Hold unmatched stops
in a fixed quarantine; do not resume them or infer ownership from their number.
Pin the task-specific pidfd and birth/TGID/group relation before its FIRST
resume. Failure/early death/missing relation is unavailable. New tracees retain
the full option mask. No thread-pidfd is treated as a process-group signal handle.

One C wait owner uses waitpid with __WALL for ptrace stops and final wait status;
pidfd poll is liveness/identity, not reaping. No waitid(P_PIDFD) is assumed for
nonleader descriptors. Distinguish synthetic bootstrap/ptrace stops and ordinary
signal-delivery stops; preserve ordinary signal delivery and never alter syscall
returns, registers, allocator memory or results. An exit-stop is not final exit.

States are Created, AwaitingRelation, PinnedStopped, ExecPending/Running,
ExitStopped and FinalReaped. Each birth gets a monotonic case-local ordinal.
Keep the old descriptor/generation until case end; a reused numeric TID gets a
new record and cannot overwrite the old one. Conflicting/unknown/out-of-order
relations refuse. EXEC former-TID/leader changes or any later exec are unavailable,
not custody repair. Explicit fatal signaling uses only the original pinned
process-leader pidfd with PIDFD_SIGNAL_THREAD_GROUP (that task's thread group),
never a POSIX process group, thread descriptor or PID/group name. Unsupported
signal flags refuse. No group/name fallback is permitted.

## Pre-exec versus target facts

At original SIGSTOP the image is the C launcher/stub. Record its actual image,
birth, group and then-visible environment as PREEXEC facts only. Prepared envp
is not the target's received environment. After the ONE intended EXEC event,
before continuation, verify the sealed target inode/image, root birth/group,
actual /proc environment allowlist/nonce/case and surviving fixture descriptors.
Verify former-TID/leader data and reject contradiction. Only then may case code
continue. The exec image/copy/hash/DSO closure assumes no foreign image writes;
path/hash/before-after checks are not mapped-page or current-role authority.

## Fixed leaves, wire and descriptor ownership

Driver creates fresh 0700/euid root, holds its original FD and writes fixed
plan.bin. C writes fixed case.bin ONCE with O_EXCL/NOFOLLOW/0600 before target
creation, after external fixture addresses are known. Child reads ONLY case.bin
relative to FD12, opened NOFOLLOW|NONBLOCK|CLOEXEC: regular/euid/nlink1/0600
checked BEFORE reading, with FD/path identity before and after,
take(limit+1), exact raw length/hash, no source discovery or replacement fallback.
Both leaves have max64KiB. Neither is a product schema or current-root proof.

Both leaves use this fixed little-endian schema1 header (256 bytes). Offsets:
0 magic[16] (exact ASCII ZERUS-G4-CASE-V1); 16 version:u16=1;
18 case:u16 (1..72, or 0x8001..0x801d for the separate helper controls);
20 nonce[16]; 36 total_length:u32; 40 root_dev:u64; 48 root_ino:u64;
56 target_sha256[32]; 88 source_sha256[32]; 120 feature_sha256[32];
152 generator_sha256[32]; 184 expected_outcome:u8; 185 fixture_kind:u8;
186 attempt_count:u8 (1 or 5); 187 flags:u8=0; 188 address1_length:u16;
190 address2_length:u16; 192 display_length:u16; 194 leaf_kind:u8
(1 plan, 2 case); 195 reserved[29]=0; 224 payload_sha256[32]. Payload is
exactly address1, address2, display, with no separators or trailing bytes.
Each address length<=A=16384, display<=64; maximum encoded size33088, still
read with a fixed64KiB+sentinel bound. Strings contain no NUL; address transports
are the accepted explicit Unix forms only. Numeric outcome/fixture-kind values
are fixed by the72-row case inventory, not supplied commands. plan.bin uses
empty address/display fields; case.bin may fill only those fixture fields and
leaf_kind/length/payload hash. All other fields must equal the original plan.
An absent fixture uses zero length and an all-zero generator hash where its
case inventory says none. Helper-control cases use the separate H01..H29
inventory below, never a guessed72-case outcome. Unknown versions/flags/values, wrong identity/hash,
oversize/short/trailing data or mismatched plan/case refuses. No arbitrary
command, profile, owner path, callback or factory field exists. Parser safety
and source equality still require the later exact implementation review.

C inherited allowlist is standard0/1/2, root12, report-write13, delegated-parent17,
original-driver-pidfd18 and sealed-target21. All other driver-created FDs are
CLOEXEC. Driver preflight refuses unknown inherited FDs within its FD64 bound.
Two-phase mapping first duplicates every source into scratch32..47 with CLOEXEC,
then dup3/dup2 maps distinct destinations, then closes scratch/unused originals.
No mapping may clobber a still-needed source. C sets its own setup FDs CLOEXEC.
The16 scratch numbers are reserved with private /dev/null placeholders before
other descriptor acquisition. The forked mapping child first closes only those
placeholders, then stages at most16 sources there; normal C retains placeholders
so later roster acquisition cannot occupy them. The driver uses the same scheme
within FD64. C reserves leaf16, delegated-parent17, driver-pidfd18, self-pidfd19
and target-ELF21; each child remaps its parent pidfd to22. No roster descriptor
is a control channel or an implicit inherited exception.

Target allowlist at exec is 0=/dev/null, 1/2=private raw streams, 12=root directory,
13=report write-only file and 14=target-to-C control WRITE end. Setup21=sealed
ELF and22=original-C-parent pidfd are CLOEXEC and close at that exec. Cgroup,
bootstrap and roster FDs never reach target code. Before exec close all other
ranges after remapping, then apply target FD128. At EXEC verify that table;
the Rust entry sets CLOEXEC on12/13/14 before native case work. No later exec.

FD13 report has one cooperating target writer. Driver holds an INDEPENDENT
read-only open of the same 0600 single-link inode, uses pread offsets and never
seeks/reads through the write OFD. Driver closes write aliases before START;
C closes its write/scratch aliases immediately after clone (no CLONE_FILES),
before target continuation. Dup/ownership is not arbitrary uniqueness proof.
Child writes bounded body then footer; parent validates final identity, size,
version, hashes, prefix/bitmap/framing and actual exit. After-prefix activity is
unmeasured. Report over64KiB, short/missing footer or wrong framing is unavailable.
Preserve bounded partial bytes plus overflow sentinel/hash/size and finite
exceptional file evidence (FSIZE8MiB); never truncate it into a green report.

## Directional control channels

Driver-to-C uses C stdin, one-way64-byte frames, max4/256bytes: REGISTER,
START_TARGET, ABORT, FINISH; exact state/order and nonce/case/sequence required.
C-to-driver uses stdout only for fixed128-byte event frames, max4096/512KiB.
An event is the64-byte control header with kind EVENT and its20 reserved bytes
repurposed as role:u8/state:u8/ordinal:u16/TID:u32/TGID:u32/birth:u64, followed
by64 payload bytes (descriptor identity or scalar status, otherwise zeros).
The final checksum field covers the entire128-byte event with itself zeroed.
Events are evidence, never a PID/pidfd custody transfer or reconstructed handle.
Target FD14 is a DIFFERENT one-way pipe: target writes, C reads. Fixed64-byte
frames, max16/1024bytes total: STARTED once, READY<=5, ATTEMPT_END<=5,
P18_REACHED once only forP18, REPORT_DONE once, ABORT once. It is not bidirectional.

Each64-byte frame is magic8/version1/kind1/case2/nonce16/sequence4,
attempt1/phase1/flags2/status4/reserved20/checksum4. All reserved bytes zero;
strict sequence1..limit, no gaps/replay. Checksum is CRC-32/ISO-HDLC over the
whole frame with its checksum field zeroed; it detects framing mistakes and
does not establish authenticity. Bootstrap and generator channels use distinct
magic/kind/state domains with this same fixed64-byte encoding. EOF before expected completion, partial
frame, wrong nonce/case, checksum/unknown kind/overflow or deadline refuses.
READY is emitted by the harness only after actual original ready/two-match facts,
never from an invented raw flag. Pipe IO occurs outside callbacks/gates. Pressure
starts only on the correct READY; no duplicate emit. Generator stdin is another
explicit one-way64-byte pipe, max4 frames, not a reverse use of FD14.

## Bounds, external children and failure inventory

C owns clone3/PIDFD creation of at most four process leaders: target, bus,
Xvfb, generator. External children are outside the target leaf, use the same
original-parent guard/pre-exec tracing/exec-image checks, and never auto-launch
services or subprocesses. Bus is --nofork with service activation absent. Exact
Xvfb/daemon/generator source/version/args/stdio and closure are pinned before code
execution; no wrapper child is silently counted zero.

That four-leader scope is CONDITIONAL, not supplied by ordinary installed Xvfb.
Independent source read of upstream xorg-server21.1.25 shows xkb/ddxLoad.c
RunXkbComp calling Popen, and os/utils.c Popen using fork and execl /bin/sh;
the command invokes xkbcomp. No selected no-subprocess Xvfb profile or binary/
source equivalence has been supplied. The ordinary profile is UNAVAILABLE, as
are all method controls/campaign cases that require it. This is an unmet
prerequisite, not a zero-descendant observation or permission to omit Xvfb,
borrow an owner display, use a guessed option or add a shell/xkbcomp whitelist.
A later qualifying no-subprocess profile needs its exact source/ELF/DSO/config
closure reviewed before activation. Allowing additional process descendants
instead changes the owner/descriptor graph and requires a new scoped design;
the64 external task records never silently authorize those process leaders.

Target remains wall20/CPU10/AS2GiB/FSIZE8MiB/FD128/core0, eight concurrent tasks
CONDITIONAL on the accepted private nonmigrating scope. Tracer per-case AS256MiB,
CPU5, FD256, core0; driver AS512MiB/CPU60 total/FD64/core0. External helper each
AS256MiB/CPU10/FD64/core0 and the same case wall deadline. Whole serial72-case
campaign<=30min including setup/cleanup; no relaxed limit or retry.

Fixed rosters:128 target-task records and64 external-task records, 128bytes each,
plus16 quarantine records of64bytes. Original leader and thread descriptor roles
are distinct. Worst-case C descriptor count is244:192 retained task pidfds,
four original leader pidfds, ten fixed descriptors (including stdio/root/leaf/
parent/self/driver/ELF/report),14 pipe/socket ends,16 scratch placeholders and
eight transient inspection/open descriptors. Source must reject before opening
outside its assigned count; no descriptor is silently excluded. No extra copy
of a leader pidfd is held under another role. Driver max64 includes all report,
command/event, own/C pidfd and scratch descriptors. Counts are design ceilings,
not an executed descriptor audit or memory claim.
The eight transient descriptors include overflow/quarantine pins and inspection
FDs, never an extra uncounted allowance. A first overflow task is recorded in
the existing16-entry quarantine with its actual creator relation, monotonic
ordinal, birth facts, task pidfd if obtained, and pending/final-wait state. The
normal128 target records remain unchanged and never recycle. Reserve one of
the eight transient descriptors for H28's single overflow task pin. Real target
overflow is detected AFTER birth by the creator event: no claim of generic
pre-clone refusal. Stop admission/continuation at once, do not resume overflow
tasks, preserve all known original handles and classify the invocation
UNAVAILABLE. Only original pinned leader signaling and known final waits may
clean it up. Missing pin, additional unmatched birth beyond quarantine/transient
capacity, contradictory relation or incomplete final wait stays UNKNOWN with
the scope retained and campaign stopped. No PID-only cleanup, slot overwrite,
additional descriptor/roster budget or empty-group success repairs it.
Overflow/refusal/unknown event stops admission. Raw aggregate<=4MiB per case,
including the512KiB event journal, target/fixture streams and driver diagnostics;
report<=64KiB. Parent drains/bounds continuously and preserves partial evidence.

Every failure records attempted/spawned/guarded/registered/observed-exec/
exit-stop/final-reaped/unknown/not-run counts separately for driver, tracer,
target and fixtures. A pending action is journaled before advancement. No missing
child/exec/result is treated as zero. Unknown early custody stops the campaign
and retains scope/group; no another case or deletion becomes success.

Cleanup uses only each original known pidfd/identity and final wait evidence.
No attach to existing owner processes, PID/name/group/cgroup.kill fallback,
parent-controller changes, migrations or subgroup creation. Empty pids counts
never substitute for reaps or original B release. Preserve all raw evidence;
remove only exact known empty leaf through its held original parent after
identity recheck. Unknown identity/member/lifetime retains it, without adoption.

## Required control-only review/window before the Rust campaign

The helper's own controls use a separately pinned no-SDK C control-target mode,
not the unreviewed Rust allocator. No control execution is admitted by this contract.
The planned inventory has 29 finite controls, one invocation each, separate
from the 72 Rust cases.
Each row names one condition; a first sticky fault is never coverage for another.

| ID | Required condition / observed outcome |
| --- | --- |
| H01 | Baseline original creation, registration, arming, single correct exec, exact final waits. |
| H02 | Driver exits before C installs its guard; no registered target progress, custody may be unknown. |
| H03 | Driver exits after C guard but before REGISTER; no target creation. |
| H04 | Driver exits after REGISTER but before target clone; existing fixture custody retained if unknown. |
| H05 | C exits after target clone but before publication; no driver PID adoption, unknown stops scope. |
| H06 | C exits before target child death-arm; original-parent recheck prevents intended exec. |
| H07 | C exits after child death-arm but before initial stop; no claimed EXITKILL interval. |
| H08 | C exits at initial stop before options; no claimed EXITKILL interval. |
| H09 | C exits after options before RELEASE; original tracee stays covered by armed EXITKILL. |
| H10 | C exits after RELEASE before exec; parent guard and armed tracing do not grant a successful case. |
| H11 | Actual initial-exec environment disagrees; target does not continue case code. |
| H12 | Permuted colliding FD sources map to the exact surviving allowlist. |
| H13 | case.bin is a symlink; NOFOLLOW refuses before target case work. |
| H14 | case.bin changes after its first identity/read; readback refuses. |
| H15 | Partial FD14 frame followed by EOF; unavailable with partial bytes retained. |
| H16 | One wrong-case frame; no case advancement. |
| H17 | One wrong-nonce frame; no case advancement. |
| H18 | One out-of-order sequence; no case advancement. |
| H19 | Eight concurrent target tasks, complete birth/pin/exit-stop/final-wait history. |
| H20 | Ninth target task creation denied by the already-admitted leaf; denial is not an omitted task. |
| H21 | Nonleader PIDFD_THREAD pin before resume, final wait and a later distinct task ordinal. |
| H22 | Nonleader exec/former-TID identity change refuses without custody repair. |
| H23 | A second exec refuses; the first exec is not silently rebound. |
| H24 | C exits after verified target exec; no case success or unobserved reap claimed. |
| H25 | Independent pread of report leaves the writer OFD cursor unchanged. |
| H26 | Short report/footer remains unavailable with exact partial evidence. |
| H27 | One owned external helper exits before its expected readiness; target does not start. |
| H28 | A fixed single-creator no-SDK control target fills128 retained normal records, then actually creates one overflow task. Creator event plus child stop reach the real post-birth refusal/quarantine/pin path; overflow task is never admitted/resumed. Invocation is UNAVAILABLE with exact actual birth/known final waits or retained UNKNOWN, never a claim that creation was prevented. |
| H29 | Explicit CONTROL-ONLY FD-fault exception: after registration but before leaf/target/ANY external fixture creation, C uses a fixed owned-int-FD holder to open its own /dev/null entries until the next real open returns EMFILE under unchanged RLIMIT256. The normal244 budget is not claimed for this invocation. Record actual initialized prefix/count/error; preserve existing control/journal/custody FDs and close only initialized holder entries once. No second fault or later branch is qualified. |

Faults are injected only by the source-pinned control mode at its named setup
boundary; they never fabricate a System allocator result or alter target SDK
state. H02..H10/H24 self-exit the corresponding OWN driver/C process using its
exact local phase gate. They do not signal a PID learned from logs. H29 cannot
alter host/shared-UID limits, and never retries the unavailable observation.
H22/H23/H29 are distinct cases, not combined post-fault coverage. Exact native
phase-gate/holder implementation and ABI proof remain required before scheduling.
H29's fixed holder has256 int slots within C's existing AS limit; it contains
only actual returned owned descriptors, never reconstructed identifiers. This
declared fault-only exception is not an increase in the normal244 ceiling,
RLIMIT256, target limits or any production budget. If actual EMFILE is not
reached and evidenced, that coverage stays UNVERIFIED. H28 exercises the actual
post-birth tracer refusal, not a cooperating source guard. It claims no generic
syscall-entry blocker, fake clone return or enlarged normal roster. A terminal
wait that cannot be completed without resuming an unadmitted overflow task stays
UNKNOWN; no unreviewed continuation or custody handoff is improvised.

Early-loss tests that produce unknown original custody STOP their own invocation
and retain that scope; they cannot be automatically followed by another case.
Any later control requires separately reviewed scheduling/known custody, not an
empty-group inference. Test-only phase gates/native-control identities/cleanup
need their exact implementation pins first; no unreviewed guardian or PID fallback.
This inventory contains proposed controls, not executed coverage or an approved
window.

Exact source/ABI/header/compiler/kernel/ELF/DSO, FD codec, signal/wait logic,
fixtures/generators/control hooks and resource enforcement need fresh G1/G3/webui
implementation reviews. Only separately scheduled native controls/raw proof
can qualify this helper, followed by a separately scheduled72-case Rust campaign.
All ClassM/physical/whole-profile/current/export/factory/consumer/writer/Notify
and owner/main gates remain unchanged. Original graph+CleanExit+join alone
releases B; current G1 exports remain absent and take priority if announced.

Primary reference semantics, not local execution:
[pinned glibc x86_64 wrapper](https://raw.githubusercontent.com/bminor/glibc/1848099f063e/sysdeps/unix/sysv/linux/x86_64/libc_sigaction.c),
[pinned glibc generic wrapper](https://raw.githubusercontent.com/bminor/glibc/1848099f063e/sysdeps/unix/sysv/linux/libc_sigaction.c),
[signal-set membership](https://man7.org/linux/man-pages/man3/sigsetops.3.html),
[parent death](https://man7.org/linux/man-pages/man2/PR_SET_PDEATHSIG.2const.html),
[thread pidfds](https://man7.org/linux/man-pages/man2/pidfd_open.2.html),
[ptrace](https://man7.org/linux/man-pages/man2/ptrace.2.html),
[clone3](https://man7.org/linux/man-pages/man2/clone.2.html),
[signal scope](https://man7.org/linux/man-pages/man2/pidfd_send_signal.2.html),
[pids](https://cdn.kernel.org/doc/html/latest/admin-guide/cgroup-v2.html#pid).
