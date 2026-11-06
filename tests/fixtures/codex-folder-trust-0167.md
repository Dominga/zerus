# Codex 0.167 folder consent captures

`codex-folder-trust-0167.txt` and
`codex-folder-trust-git-root-0167.txt` were captured from the official Linux
Codex 0.167.0 binary on 2026-11-06, in a private 100×40 tmux terminal. HOME,
CODEX_HOME, XDG directories and the tmux socket were isolated. A keyless
localhost provider was configured; no model request was made. The ordinary
folder had a harmless `.codex/config.toml`; the Git case used a newly created
synthetic repository and its subdirectory. Temporary path prefixes were
replaced with `/work`. Native prose, wrapping, choices and footer are unchanged;
blank terminal padding was removed.

Selecting **Back to Agent Command Center** showed the live `Agent command center`
screen, left the same process running and left the complete configuration bytes
unchanged, with no saved project trust. The Git root was not trusted either.
A private Codex 0.166.0 control displayed **Quit** and `esc quit` and exited when
selected. Zerus never makes either choice automatically.

The behavior matches upstream `rust-v0.167.0`:

- `codex-rs/tui/src/onboarding/trust_directory.rs` pairs
  `TrustCancelAction::AgentsOverview` with the Back label and `esc back`.
- `codex-rs/tui/src/onboarding/directory_trust.rs` returns the cancelled consent.
- `codex-rs/tui/src/lib.rs` switches to `SessionSelection::AgentsOverview`,
  clearing the pending CLI prompt and images without persisting folder trust.

Unit tests also apply the new label/footer pair to the existing native Windows
drive/Git-root captures. Those combinations are synthetic parser controls, not
new Windows captures. The Windows real-agent suite separately exercises actual
folder consent through the private session host and keyless provider.
