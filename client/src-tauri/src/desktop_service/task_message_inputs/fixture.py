# Private transport prefix for the held-input review client; the remainder is
# the existing service fixture. It records every listing and release request
# and answers from fixture.json.
import sys
if sys.argv[1:] == ['--help']:
    import json
    from pathlib import Path
    c = json.loads((Path(__file__).parent / 'fixture.json').read_text())
    if c.get('review_help'):
        print('Desktop task message input review: desktop-task-message-input-review-v1', flush=True)
elif sys.argv[1:3] in [['desktop', 'frontend-request'], ['desktop', 'request']]:
    import io
    import json
    line = sys.stdin.readline()
    sys.stdin = io.StringIO(line)
    value = json.loads(line)
    request = value.get('request', value)
    if request.get('op') in ['task_message_inputs', 'task_message_input_release']:
        from pathlib import Path
        root = Path(__file__).parent
        c = json.loads((root / 'fixture.json').read_text())
        with (root / 'review-calls').open('a') as log:
            log.write(json.dumps({'args': sys.argv[1:], 'value': value}) + '\n')
        if request['op'] == 'task_message_inputs':
            page = dict(abi='desktop-task-message-v1', scope=request['scope'], read_only=True,
                        entries=c.get('entries', []), next=c.get('next'), code=c.get('page_code', ''))
            print(json.dumps(page))
            sys.exit(0)
        if sys.argv[2] != 'frontend-request':
            # As the service does: a native release only on its own lane.
            print(json.dumps(dict(request_id=None, accepted=False, durable=False,
                                  code='frontend_authentication_required')))
            sys.exit(0)
        if c.get('release_lost'):
            sys.exit(1)
        release = request['release']
        print(json.dumps(dict(abi='desktop-task-message-v1', release_id=release['release_id'],
                              request_id=release['request_id'], scope=release['scope'],
                              outcome=c.get('outcome', 'removed'), code=c.get('code', ''))))
        sys.exit(0)
