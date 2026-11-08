#!/bin/sh
# Fake hgs for task_details::tests (a deep link's short reference): records
# its arguments, waits `sleep` seconds when that file exists, then prints `reply`.
here=$(dirname "$0")
echo "$*" >> "$here/calls"
cat >/dev/null
[ -f "$here/sleep" ] && sleep "$(cat "$here/sleep")"
cat "$here/reply"
