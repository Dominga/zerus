Run a routine command outside your sandbox (hgs guide team-run)

Zerus runs these built-in templates for a team member, outside the member's
sandbox, in the member's own folder, with a bounded time and output:
  cargo-build {package?, toolchain?}  cargo build --locked [--package=P]
  cargo-test {filter?, bin?, package?, toolchain?}
                             cargo test --locked [--package=P] [--bin=B] [F]
  cargo-clippy {package?, toolchain?}  clippy --all-targets -- -D warnings
  qt-configure               cmake configure of tray/ into .ci-build/gui
  qt-build {targets?}        cmake --build .ci-build/gui
  qt-test {suite, functions?, shots?}  a test binary from .ci-build/gui,
                             offscreen; shots go to .ci-build/shots
  android-test               mobile/android :app:testDebugUnitTest
  clean-build {paths}        removes build outputs (.ci-build, build, target)
  git-merge-lead             git merge --no-edit the lead's branch, in your
                             own worktree or clone, never a shared team folder
  git-commit {message, paths?}  Zerus commits your work on your branch
On Windows the Qt templates load Visual Studio and Qt; do not wrap them in
your own vcvars batch. Shots stay in your folder: name their path in the report.
Codex on Windows: its sandbox keeps .git read-only, so commit only with
git-commit; the Store pwsh cannot start there: use powershell.exe.
Ask for one as the member (identity as report; a sandbox needs no approval):
  {"request_id":"UUID","team":"ID","attempt":"ATTEMPT",
   "template":"cargo-test","args":{"bin":"hgs","filter":"teams"}} | hgs team run --json
Use a new request_id for every run; --wait returns the exit code when ready
(90 s), without it "queued". Ask again with the same request_id to read the
result; it never runs the command twice. Values are typed; a path outside your
folder or a shell metacharacter is refused. The owner may turn the set off or
add fixed templates.
A pipeline stage (review or test) hand-out names your stage attempt: answer
with a verdict (hgs team report --json); edit none of its files. Command and
land stages are Zerus'. A role's context_before_task may compact or clear your
context before a hand-out, which then carries the full task text.
Build broker on: hgs guide team-broker.
Anything else still asks the lead.
