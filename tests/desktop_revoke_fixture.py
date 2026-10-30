REVOCATION_CLI = r'''#!{python}
import json, os, pathlib, sys, time
root = pathlib.Path(os.environ['TRAY_FIXTURE'])
args = sys.argv[1:]
mode = os.environ.get('REVOCATION_MODE', 'settled')
configuration = 'fixture-configuration'


def log(request_id=None):
    with (root / 'revocation.jsonl').open('a') as record:
        record.write(json.dumps({{'args': args, 'request_id': request_id}}) + '\n')


def settled(request_id, config=configuration):
    return {{'request_id': request_id, 'configuration': config,
            'accepted': True, 'durable': True, 'revoked': True, 'checkpointed': True}}


if args == ['--version']:
    print('hgs 0.0-fixture')
    raise SystemExit(0)
if args == ['--help']:
    log()
    if mode != 'unsupported':
        print('Desktop revoke ABI: desktop-revoke-v1')
    raise SystemExit(0)
if args == ['desktop', 'status', '--json']:
    log()
    print(json.dumps({{'configuration': configuration, 'running': False}}))
    raise SystemExit(0)
if args[:2] == ['desktop', 'revoke']:
    request_id = args[args.index('--request-id') + 1]
    log(request_id)
    if mode == 'unsupported':
        raise SystemExit(99)
    if mode == 'held':
        print('another desktop service is active', file=sys.stderr)
        raise SystemExit(1)
    if mode == 'disconnect':
        raise SystemExit(1)
    if mode == 'timeout':
        time.sleep(5)
    if mode == 'wrong_config':
        print(json.dumps(settled(request_id, 'other-configuration')))
    elif mode == 'noop':
        print(json.dumps({{'request_id': request_id, 'configuration': configuration,
                          'applicable': False, 'handoff': 'none', 'revoked': True, 'nothing_written': True}}))
    elif mode == 'malformed':
        print('{{')
    elif mode == 'pending':
        print(json.dumps({{'request_id': request_id, 'configuration': configuration,
                          'accepted': False, 'revoked': False, 'checkpointed': False,
                          'code': 'revocation_pending'}}))
    elif mode == 'resources':
        print(json.dumps({{'request_id': request_id, 'configuration': configuration,
                          'accepted': True, 'durable': True, 'revoked': False, 'checkpointed': False,
                          'code': 'operation_resources_not_drained',
                          'resource_custody': {{'attempt_id': '33333333-3333-4333-8333-333333333333',
                            'helper_pid': 7654321, 'helper_state': 'running', 'revoke_request_id': request_id}}}}))
    else:
        print(json.dumps(settled(request_id)))
    raise SystemExit(0)
if args[:2] == ['desktop', 'request']:
    request = json.load(sys.stdin)
    log(request['request_id'])
    if mode == 'disconnect':
        print(json.dumps(settled(request['request_id'])))
    elif mode == 'malformed':
        print('{{')
    else:
        print(json.dumps({{'request_id': request['request_id'], 'configuration': configuration,
                          'accepted': False, 'revoked': False, 'checkpointed': False,
                          'code': 'revocation_pending'}}))
    raise SystemExit(0)
raise SystemExit(2)
'''
