#!/bin/sh
# A fake `hgs` for src/speech_settings/tests.rs. It runs through a symlink in
# a test's folder and keeps its log there: the arguments of every call in
# `calls`, a call's standard input in `payload`. A `legacy` file there makes
# the help lack provider management.
dir=$(dirname "$0")
printf '%s\n' "$*" >> "$dir/calls"
case "$*" in
  *--help*)
    printf '  speech --json  Speech input\n'
    [ -f "$dir/legacy" ] || printf 'Speech profile management v1\n' ;;
  *)
    cat > "$dir/payload"
    printf '{"schema":1,"state":"completed","client_dispatch_started":true}' ;;
esac
