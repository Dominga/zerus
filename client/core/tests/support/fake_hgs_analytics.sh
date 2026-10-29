#!/bin/sh
# Fake hgs for analytics::tests::an_older_cli_is_only_asked_for_help_and_errors_never_echo_its_text.
here=$(dirname "$0")
echo "$* session=${HGS_SESSION:-unset} run=${HGS_RUN_ID:-unset}" >> "$here/calls"
case "$*" in *--help*) cat "$here/help"; exit 0;; esac
cat >/dev/null
if [ -f "$here/fail" ]; then
  echo private-stderr-sentinel >&2
  exit 1
fi
cat "$here/reply"
