Run a routine command outside your sandbox (hgs guide team-run)

Zerus runs these built-in templates for a team member, outside the member's
sandbox, in the member's own folder, with a bounded time and output:
  cargo-build {package?, toolchain?}  cargo build --locked [--package=P]
  cargo-test {filter?, bin?, package?, toolchain?}
                             cargo test --locked [--package=P] [--bin=B] [F]
  cargo-clippy {package?, toolchain?}  clippy --all-targets -- -D warnings
  qt-configure               cmake configure into .ci-build/gui
  qt-build {targets?}        cmake --build .ci-build/gui
  qt-test {suite, functions?, shots?}  a test binary from .ci-build/gui,
                             offscreen; shots go to .ci-build/shots
  android-test               mobile/android :app:testDebugUnitTest
  clean-build {paths}        removes build outputs (.ci-build, build, target)
  git-merge-lead             git merge --no-edit the lead's branch, in your
                             own worktree or clone, never a shared team folder
  git-commit {message, paths?}  Zerus commits your work on your branch
On Windows the Qt templates load Visual Studio and Qt themselves: never wrap
them in your own vcvars batch file. Shots stay in your folder: name their
path in the report; the desktop shows them from there.
Ask for one as the member (identity as report; a sandbox needs no approval):
  {"request_id":"UUID","team":"ID","attempt":"ATTEMPT",
   "template":"cargo-test","args":{"bin":"hgs","filter":"teams"}} | hgs team run --json
Use a new request_id for every run. --wait returns the exit code as soon as it
is ready (90 s); without it the answer is "queued". Ask again with the same
request_id to read the result: it never runs the command twice. A detached
runner executes it and your session shows as Building. Values are typed: an
unknown template, a value outside its type, a path outside your folder or a
shell metacharacter is refused. What you build is your own code: it runs
outside the sandbox by the owner's rule. The owner turns the set off in
Settings > Teams and may add fixed templates in Zerus state. Anything else
still asks the lead.
