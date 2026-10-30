# Bounded legacy listing decoder

`client/core::legacy::listing::decode` incrementally reads an older CLI's bulk
`ls --json` response. It invokes a background sink with provisional pages and
returns bounded metadata only after complete valid JSON and actual EOF. It does
not retain the sessions array or a per-session map. Existing GUI legacy reads
and `legacy::Schedule` are unchanged in this slice. No service source starts.

The raw adapter will use that same schedule and spool pages privately through
the host's existing private-file helpers. It must discard staging on **any**
decoder failure or unsuccessful owned-child exit. Earlier page callbacks are
not completion, source freshness, eligible migration evidence or writer
authority. The source adapter, child lifetime and spool admission are the next
integration slice; this decoder creates no files, threads or processes.

The decoder retains one page, one current row and bounded metadata. Pages target
512 KiB and at most 128 rows. A larger legal singleton fits the existing 1 MiB
watch frame with 4 KiB envelope headroom. Limits refuse above 64 MiB input,
250,000 rows, 128 top-level fields, 256-byte metadata keys or 512 KiB of charged
metadata. Callers may lower input/row limits. A 1 MiB read window bounds each
decoded row/value before a large string or array can accumulate the full input.
The fixed 16 KiB read buffer is additional memory; decoded JSON allocation and
whole-service RSS still require qualification.

Rows use the shared watch catalog key and preserve unknown fields, conversation
identity and background counts. Invalid row identity, duplicate top-level keys,
missing/wrong-shaped sessions, truncation and trailing input refuse the whole
listing. Duplicate session keys are left to the indexed projection's uniqueness
check, without another resident set. Metadata ordering is unrestricted.

Cancellation and the supplied deadline are checked before input, at each value,
periodically while consuming bytes, and before success. The executor must still
enforce the owned child's read/exit deadline: a parser cannot interrupt an
arbitrary `Read` already blocked in the OS. A one-byte lookahead distinguishes a
real EOF at the byte limit from hidden trailing input. Errors expose static
categories and do not include source bodies or staging details.

Focused tests generate 100,000 rows incrementally, consume 782 bounded pages,
preserve metadata after sessions, and cover exact byte limits, oversized values,
metadata bounds, malformed/partial input, cancellation, deadline and sink failure.
