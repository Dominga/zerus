Run a routine command outside your sandbox (hgs guide team-run)

Zerus runs these built-in templates for a team member, outside the member's
sandbox, in the member's own folder, with a bounded time and output:
  cargo-build {filter?}      cargo build --locked (filter: a package name)
  cargo-test {filter?}       cargo test --locked
  cargo-clippy {filter?}     cargo clippy --locked --all-targets -- -D warnings
  qt-configure               cmake configure into .ci-build/gui
  qt-build {targets?}        cmake --build .ci-build/gui
  qt-test {suite, functions?, shots?}  a test binary from .ci-build/gui,
                             offscreen; shots go to .ci-build/shots
  git-merge-lead             git merge --no-edit teams-v2/lead
  git-commit {message, paths?}  Zerus commits your work on your branch
Ask for one as the member (identity as report; a sandbox needs no approval):
  {"request_id":"UUID","team":"ID","attempt":"ATTEMPT",
   "template":"cargo-test","args":{"filter":"hgs"}} | hgs team run --json
It answers with the exit code and the last 4000 characters of output, and
records the run on the attempt. Values are typed: an unknown template, a
value outside its type, a path outside your folder or a shell metacharacter is
refused. What you build is your own code: cargo, cmake and qt-test run your
build scripts, tests, CMakeLists and .cargo/config outside the sandbox, by the
owner's rule. The owner turns the set off for every team in Settings > Teams,
and may add fixed templates (their own program and argv) in Zerus state; those
are listed as the owner's additions. Anything else still asks the lead.
