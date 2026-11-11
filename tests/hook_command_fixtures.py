"""Personal shell commands that hook migration must preserve, in private homes."""
import json
import shlex
import subprocess
import tomllib


def runner(path, hgs):
    path.write_text(
        '#!/bin/sh\n'
        'if [ "${HOOK_PROBE_ONLY:-}" = 1 ] && [ "$1" = __state ]; then exit 0; fi\n'
        'exec ' + shlex.quote(str(hgs)) + ' "$@"\n'
    )
    path.chmod(0o755)


def composed(root, program):
    q = shlex.quote(str(program))
    folder = shlex.quote(str(root))
    return [
        ('cd_and', f'cd {folder} && {q} __state hook'),
        ('test_and', f'test -x {q} && {q} __state hook'),
        ('assignment', f'FOO=1 {q} __state hook'),
        ('comment', f'{q} __state hook # personal note'),
        ('channel_comment', f'{q} __state answer-channel # personal note'),
        ('double_quoted', f'"{program}" __state hook'),
        ('single_quoted', f"'{program}' __state hook"),
        ('quoted_cd_and', f'cd "{root}" && "{program}" __state hook'),
        ('quoted_assignment', f'FOO="a b" "{program}" __state hook'),
    ]


def append(path, agent, probes):
    if agent == 'kimi':
        with path.open('a') as output:
            for index, (_, command) in enumerate(probes):
                output.write('\n[[hooks]]\nevent = "Stop"\ncommand = ' + json.dumps(command) + f'\ntimeout = {17 + index}\n')
    else:
        doc = json.loads(path.read_text())
        doc['hooks']['Stop'].extend(
            {'hooks': [{'type': 'command', 'command': command, 'timeout': 17 + index}]}
            for index, (_, command) in enumerate(probes)
        )
        path.write_text(json.dumps(doc))


def commands(path, agent, count):
    if agent == 'kimi':
        pairs = [(h['event'], h['command']) for h in tomllib.loads(path.read_text())['hooks']]
    else:
        pairs = [(event, h['command']) for event, groups in json.loads(path.read_text())['hooks'].items()
                 for g in groups for h in g['hooks']]
    return [command for event, command in pairs if event == 'Stop'][-count:]


def preserves_and_runs(test, before, after, env):
    test.assertEqual(len(after), len(before), "personal hook entries must not disappear")
    for (label, original), actual in zip(before, after, strict=True):
        with test.subTest(command=label):
            test.assertEqual(actual.encode(), original.encode(), 'personal shell bytes must remain exact')
            result = subprocess.run(['bash', '-c', actual], env=dict(env, HOOK_PROBE_ONLY='1'),
                                    input='', text=True, capture_output=True, timeout=8)
            test.assertEqual(result.returncode, 0, result.stderr)
