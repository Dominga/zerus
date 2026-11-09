# Native text editing regression

This renders the production Composer and its real optimistic draft store, plus
shared input/textarea fields. Only surrounding services, chips and bridge replies
are synthetic. It does not start an agent or send a message. The browser engine
owns undo history; no JavaScript history implementation is substituted.

From the repository root, install the client's locked dependencies and build:

```sh
npm --prefix client ci
node client/node_modules/vite/bin/vite.js build \
  --config client/bench/text-edit/vite.config.ts --outDir /path/to/private/rendered
```

Run `client/scripts/text-edit-smoke.py /path/to/private/rendered` **only inside a
fresh private Xvfb and D-Bus session**, with `GDK_BACKEND=x11`,
`WAYLAND_DISPLAY=zerus-none` and private HOME/XDG directories. It needs GTK3,
WebKitGTK 4.1, PyGObject and libXtst. It hosts the built page on loopback, uses an
ephemeral WebKit context and sends XTest keys only to its own window. Its clipboard
contains synthetic text on that private display. It destroys its window and
settles its HTTP server at exit; the check has a 25-second bound.

Checks cover typing, Tab suggestion acceptance, delayed model echoes, native
multiline insertion, dialog updates, context/history reset and composition-event
ordering. The driver additionally exercises real Ctrl+Z, Ctrl+Shift+Z, Ctrl+Y,
Cut and Paste while checking both DOM text and the draft store. Composition
ordering is simulated; this does not qualify a particular operating-system IME
or microphone. Session voice delivery still uses its existing send/preview
contract, rather than inserting text into a draft.

Changing composer handle/text epoch starts another history. Ordinary edits and
same-value replies retain the current native history. The macOS Edit menu is
unchanged; a WebKitGTK result is not native WKWebView or WebView2 qualification.
