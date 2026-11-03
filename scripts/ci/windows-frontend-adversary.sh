#!/usr/bin/env bash
# Fixed native parents only. A misspelled/compiled-out filter is never a pass.
set -euo pipefail
cd "$(dirname "$0")/../.."
if [ "${GITHUB_ACTIONS:-}" != true ] || [ "${RUNNER_ENVIRONMENT:-}" != github-hosted ]; then
    echo 'Native frontend adversary requires a disposable hosted Windows runner' >&2
    exit 2
fi
if [ "$#" -ne 1 ]; then
    echo 'Exactly one closed fixture mode is required' >&2
    exit 2
fi
case "$1" in
    core)
        args=(--manifest-path client/core/Cargo.toml --test windows_frontend_server)
        selected=hosted_same_user_frontend_impostors_are_unknown
        ;;
    actor)
        args=(--bin hgs)
        selected=desktop::notifications::activation::tests::native_refused_frontend_keeps_the_original_launch_unspent
        ;;
    frontend|frontend-replay)
        args=(--manifest-path client/src-tauri/Cargo.toml --lib)
        if [ "$1" = frontend-replay ]; then args+=(--no-default-features); fi
        selected=instance::adversarial::native_lower_integrity_server_never_forwards_or_claims_a_gui
        ;;
    *) echo 'usage: windows-frontend-adversary.sh core|actor|frontend|frontend-replay' >&2; exit 2 ;;
esac
mkdir -p artifacts/test-results
log="artifacts/test-results/windows-frontend-adversary-$1.log"
CARGO_TERM_COLOR=never cargo +1.99.1 test --locked "${args[@]}" "$selected" -- \
    --ignored --exact --nocapture --test-threads=1 --color=never 2>&1 | tee "$log"
python - "$log" "$selected" <<'PY'
import sys
from pathlib import Path
with Path(sys.argv[1]).open('rb') as stream:
    raw = stream.read((1 << 20) + 1)
if len(raw) > 1 << 20:
    raise SystemExit('Native result exceeds the evidence read bound')
text = raw.decode('utf-8')
if (f'test {sys.argv[2]} ...' not in text or
        'test result: ok. 1 passed; 0 failed; 0 ignored;' not in text):
    raise SystemExit('The exact native parent did not complete; zero tests is not proof')
PY
