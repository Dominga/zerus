#!/bin/sh
# Fake hgs for approvals::tests::only_the_journal_s_reads_run_and_oversized_output_fails_as_intended.
here=$(dirname "$0")
echo "$*" >> "$here/calls"
cat >/dev/null
case "$*" in
  --help) cat "$here/help";;
  'approval-log list')
    if [ -f "$here/fail" ]; then
      cat "$here/fail" >&2
      cat "$here/reply"
      exit 3
    fi
    cat "$here/reply";;
  'approval-log detail') echo '{"status":"approved","detail":""}';;
  *) exit 9;;
esac
