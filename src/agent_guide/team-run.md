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
Ask for one as the member (identity as report; a sandbox needs no approval):
  {"request_id":"UUID","team":"ID","attempt":"ATTEMPT",
   "template":"cargo-test","args":{"bin":"hgs","filter":"teams"}} | hgs team run --json
Use a new request_id for every run; --wait returns the exit code when ready
(90 s), without it "queued". Ask again with the same request_id to read the
result; it never runs the command twice. Values are typed: an unknown template,
a value outside its type, a path outside your folder or a shell metacharacter is
refused. What you build is your own code: it runs outside the sandbox by the
owner's rule. The owner turns the set off in Settings > Teams and may add fixed
templates in Zerus state.
In a pipeline stage (review or test) the hand-out names your stage attempt;
answer it with a verdict (hgs team report --json); edit none of its files.
Command and land stages are Zerus', with no member. A role's context_before_task
may compact or clear your context before a hand-out: it then carries the full
task text.
With the owner's build broker (members sharing one folder) a build template
runs in your own snapshot of the folder: name the files you change,
"args":{"files":["src/a.cpp"]}; the rest builds at HEAD. A failure in another
member's file is theirs: wait and build again, never edit their files.
Anything else still asks the lead.
