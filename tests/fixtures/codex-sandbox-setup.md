# Codex Windows sandbox setup

The three-choice picker is independent of folder trust. Reviewed against
[Codex rust-v0.169.0](https://github.com/openai/codex/tree/d32063f96c2dff1dc96af839b3952abb6acfeda4):
`codex-rs/tui/src/chatwidget/windows_sandbox_prompts.rs`,
`bottom_pane/list_selection_view.rs`, `selection_row_links.rs`, and
`bottom_pane/mod.rs`, and `history_cell/session.rs`.

`SelectionViewParams::picker()` uses `SelectionRowDisplay::Wrapped`. The header
uses the pane width minus four columns, with two columns of left padding.
Option rows use the full pane width and a five-column marker/number gutter;
continuation rows retain that gutter. Wrapping can split the documentation URL.
The complete native text is required: neither `...` nor `…` replaces any suffix.
The default list keymap renders `enter select · esc back`. A different keymap,
managed two-choice variant, clipped header or label, extra row, missing option,
or ambiguous selection stays in Terminal.

The native session header has a title and directory; only YOLO mode adds the
exact `permissions: YOLO mode` row. The normal 80-column fixture omits it.

The 80- and 48-column fixtures are synthetic layouts from those render rules,
with a reserved project path and the second option highlighted. The older
`codex-sandbox-setup-windows.txt` is the existing synthetic wide-screen capture.
The hosted Windows native fixture retains its actual latest-release screen as
`artifacts/test-results/codex-sandbox-setup-native.txt`.

The Activity card preserves native labels and disclosure literally; its app
heading and setup guidance are translated. The highlight is not a default
answer. Only an explicit person submission uses the existing answer receipt
and guarded native navigator. Agent callers are refused before reservation and
each navigation snapshot rechecks the person-only boundary. A setup selection acknowledges Codex's native
setup status and opens Terminal; Zerus sends no further setup or Administrator
input. Without a writable terminal the card remains read-only. New pending
input or changed run, process, pane or frame refuses the old card, and an
uncertain answer is never replayed.

Before any remote SSH or capability probe, an agent caller's `codex-sandbox:`
answer is refused too, including guarded and `__state` routes. The fake-SSH
controls retain person answers and byte-for-byte forwarding of every other
valid answer. Inspected answer input retains the native 128 KiB limit and the
Windows overlapped relay and interrupt handling.

Private synthetic terminal controls cover both setup selections, native-status
acknowledgement, Quit, read-only copy mode, stale bindings and receipt replay.
The hosted Windows native case explicitly overrides the account's bypass default
with workspace-write/on-request launch switches and
explicitly selects only Quit; it neither extracts
nor runs the sandbox setup executable, and proves no setup/elevation behavior.
