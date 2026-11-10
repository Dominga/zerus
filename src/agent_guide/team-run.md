Run a routine command outside your sandbox (hgs guide team-run)

Zerus runs these built-in templates for a member, outside its sandbox, in the
member's folder, with bounded time and output:
  cargo-build/test/clippy {package?, filter?, bin?, toolchain?}
                             cargo build/test/clippy --locked, typed filters
                             cmake in .ci-build/gui; offscreen shots there
  tauri-build/dev/launch {files?}  app build/dev/launch with 273's dev identity;
                             launch refuses a switch build outside the package
  android-test               mobile/android :app:testDebugUnitTest
  clean-build {paths}        removes .ci-build, build, target
  git-merge-lead             merge the lead's branch in your own worktree
  git-commit {message, paths?}  commits your work on your branch
  cmake-*, ctest, msbuild-build, vstest: hgs guide team-cpp
Qt templates load Visual Studio and Qt; do not wrap them in vcvars. Shots stay
in your folder: name their path in the report.
Tauri app builds/dev/launch go only through tauri-*: they use the
development identity and own single-instance name, data and WebView2 folders, so
they never raise or block the owner's Zerus. Only release-candidate/package builds
the switch identity; never name tauri.windows.switch.conf.json in a member build.
Codex on Windows: .git is read-only; commit with git-commit, use powershell.exe.
Ask for one as the member (identity as report; a sandbox needs no approval):
  {"request_id":"UUID","team":"ID","attempt":"ATTEMPT",
   "template":"cargo-test","args":{"bin":"hgs","filter":"teams"}} | hgs team run --json
Use a new request_id for every run; --wait returns the exit code when ready
(90 s), without it "queued". Ask again with the same request_id to read the
result; it never runs twice. Values are typed; a path outside your folder or a
shell metacharacter is refused. The owner may turn the set off or add templates.
A pipeline stage hand-out names your stage attempt: answer with a verdict
(hgs team report --json); edit none of its files. Command and land stages are
Zerus'. A role's context_before_task may compact or clear your context before a
hand-out, which then carries the full task text.
Build broker on: hgs guide team-broker.
Anything else still asks the lead.
