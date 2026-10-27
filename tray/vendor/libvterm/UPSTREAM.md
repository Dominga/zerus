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
