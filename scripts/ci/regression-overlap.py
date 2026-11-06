#!/usr/bin/env python3
"""Avoid starting a second full matrix while this SHA's candidate verifies it."""
import json
import os
from pathlib import Path
import re
import time
import urllib.request

REPOSITORY = 'ufna/zerus'


def candidate_cover(fetch, commit):
    """Read-only scheduling evidence, never a successful qualification receipt."""
    if not re.fullmatch(r'[0-9a-f]{40}', commit):
        raise ValueError('Invalid source commit')
    examined = 0
    for workflow, events in [('release.yml', {'workflow_dispatch'}), ('nightly.yml', {'workflow_dispatch', 'schedule'})]:
        runs = fetch(f'actions/workflows/{workflow}/runs?head_sha={commit}&status=in_progress&per_page=20')
        if runs['total_count'] > 20:
            raise ValueError('Candidate listing exceeds its bound')
        for run in runs['workflow_runs']:
            if not (run['head_sha'] == commit and run['head_branch'] == 'main' and
                    run['head_repository']['full_name'] == REPOSITORY and run['event'] in events and
                    run['path'] == '.github/workflows/' + workflow and run['status'] == 'in_progress'):
                continue
            examined += 1
            if examined > 4:
                raise ValueError('Candidate job reads exceed their bound')
            jobs = fetch(f"actions/runs/{int(run['id'])}/jobs?per_page=100")
            if jobs['total_count'] > 100:
                raise ValueError('Candidate job listing exceeds its bound')
            verify = [j for j in jobs['jobs'] if j['name'].startswith('verify / ')]
            gates = [j for j in verify if j['name'] == 'verify / Suite results']
            if len(gates) == 1 and gates[0]['status'] == 'completed':
                if gates[0]['conclusion'] == 'success':
                    return int(run['id'])
                continue  # A failed candidate does not suppress independent regression.
            if any(j['status'] in {'queued', 'in_progress'} for j in verify):
                return int(run['id'])
    return None


def main():
    end = time.monotonic() + 60

    def fetch(path):
        remaining = end - time.monotonic()
        if remaining <= 0:
            raise TimeoutError('Candidate read deadline')
        request = urllib.request.Request('https://api.github.com/repos/' + REPOSITORY + '/' + path,
                                         headers={'Authorization': 'Bearer ' + os.environ['GH_TOKEN'],
                                                  'Accept': 'application/vnd.github+json'})
        with urllib.request.urlopen(request, timeout=min(10, remaining)) as response:
            data = response.read(2 * 1024 * 1024 + 1)
        if len(data) > 2 * 1024 * 1024:
            raise ValueError('Candidate response bound')
        return json.loads(data)

    try:
        covered = candidate_cover(fetch, os.environ['SOURCE_COMMIT'])
        reason = 'candidate-verification' if covered else 'no-active-candidate'
    except (OSError, ValueError, KeyError, TypeError):
        covered, reason = None, 'candidate-observation-unavailable'
    # Incomplete API evidence runs regression; it never fabricates coverage.
    record = dict(run_checks=covered is None, reason=reason, candidate_run=covered)
    print(json.dumps(record))
    with Path(os.environ['GITHUB_OUTPUT']).open('a') as output:
        output.write('run_checks=' + str(record['run_checks']).lower() + '\n')
        output.write('candidate_run=' + (str(covered) if covered else '') + '\n')


if __name__ == '__main__':
    main()
