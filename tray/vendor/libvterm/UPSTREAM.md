Vendored libvterm 0.3.3

Source: https://www.leonerd.org.uk/code/libvterm/libvterm-0.3.3.tar.gz
SHA-256: 09156f43dd2128bd347cbeebe50d9a571d32c64e0cf18d211197946aff7226e0

Original C sources and public headers, unmodified except for the local patch
below. MIT license: LICENSE.
Used as the ANSI/VT parser and terminal state machine. Builds offline.

## Local patches

1. `src/screen.c`, `resize_buffer()`, the `new_row_start < 0` branch: the
   cursor test reads `old_cursor.row <= old_row_end` instead of `<`. When a
   resize reflows a long wrapped line taller than the new screen and the cursor
   sits on that line's last row, upstream never placed the cursor and aborted
   with "screen_resize failed to update cursor position". The cursor now goes
   to row 0, as for the line's other rows. Covered by
   `reflowedLongLineKeepsTheCursorInside` in `tray/tests/test_terminalview.cpp`.
2. `src/screen.c`, `resize_buffer()`, the walk back over continuation rows
   stops at row 0 (`old_row > 0`, upstream `>= 0`). When the top row of the
   screen continues a line whose head is in the scrollback, upstream walked to
   row -1 and copied cells from before the buffer: the desktop crashed with an
   access violation on a resize (owner, 2026-10-28, a 261-column terminal).
   Covered by `reflowFromAContinuedTopRowStaysInsideTheBuffer`.
3. `src/screen.c`, `resize_buffer()`: a cursor the reflow did not place no
   longer aborts the process ("screen_resize failed to update cursor
   position", a 0xc0000409 crash on 2026-10-27); it goes inside the screen.
