#!/bin/sh
# Fake hgs for worktree_policy::tests::output_past_128_kib_is_refused.
here=$(dirname "$0")
echo "$*" >> "$here/calls"
case "$*" in *--help*) cat "$here/help"; exit 0;; esac
cat >/dev/null
cat "$here/reply"
