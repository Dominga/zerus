# Terminal copy and paste keys

This renders two real xterm.js terminals in the engine under test. One runs
Terminal's copy key handler and right-click menu (`src/ui/sessions/terminal-clipboard.ts`);
the other runs xterm alone, as a recorded baseline. Each records what would
reach the agent, the handler's copies, and the engine's own copy, paste and
context-menu events. No session, bridge or clipboard write is involved.

From the repository root, install the client's locked dependencies and build:

```sh
npm --prefix client ci
node client/node_modules/vite/bin/vite.js build \
  --config client/bench/terminal-keys/vite.config.ts --outDir /path/to/private/rendered
```

Run `client/scripts/terminal-keys-smoke.py /path/to/private/rendered` **only inside
a fresh private Xvfb and D-Bus session**, with `GDK_BACKEND=x11`,
`WAYLAND_DISPLAY=zerus-none` and private HOME/XDG directories. It needs GTK3,
WebKitGTK 4.1, PyGObject and libXtst. It hosts the built page on loopback, uses an
ephemeral WebKit context, and sends XTest keys and a right-click only to its own
window. Its clipboard holds synthetic text on that private display. The check
has a 25-second bound.

It requires, in Terminal:

- Ctrl+Shift+C copies and sends nothing to the agent.
- Ctrl+Shift+V and Shift+Insert reach the engine's paste event, which Terminal
  handles, and send nothing directly.
- Ctrl+C and Ctrl+V reach the agent as `^C` and `^V`.
- Right-click on a selection offers Copy selection and Paste.

The baseline shows that xterm alone copies nothing on Ctrl+Shift+C. A
WebKitGTK result is not native WKWebView or WebView2 qualification.
