# Private transport prefix for the task message client; the remainder is the
# existing service fixture. It records every task message request and models
# only a sent original whose admission reply is lost.
import sys
if sys.argv[1:] == ['--help']:
    import json
    from pathlib import Path
    c = json.loads((Path(__file__).parent / 'fixture.json').read_text())
    if c.get('message_help_fail'):
        sys.exit(1)
    if not c.get('message_no_abi'):
        print('Desktop task message ABI: desktop-task-message-v1', flush=True)
    if c.get('message_references'):
        print('Desktop task message references: desktop-task-message-reference-v1', flush=True)
elif sys.argv[1:3] in [['desktop', 'frontend-request'], ['desktop', 'request']]:
    import io
    import json
    line = sys.stdin.readline()
    sys.stdin = io.StringIO(line)
    value = json.loads(line)
    request = value.get('request', value)
    if request.get('op') == 'task_message_staging':
        from pathlib import Path
        root = Path(__file__).parent
        c = json.loads((root / 'fixture.json').read_text())
        with (root / 'message-calls').open('a') as log:
            log.write(json.dumps({'args': sys.argv[1:], 'value': value}) + '\n')
        if c.get('message_staging'):
            print(json.dumps(dict(abi='desktop-task-message-v1', scope=request['scope'],
                                  read_only=True, staging=str(root / 'staging'))))
        else:
            print(json.dumps(dict(abi='desktop-task-message-v1', read_only=True,
                                  code='task_message_staging_unavailable')))
        sys.exit(0)
    if request.get('op') in ['task_message', 'task_message_result']:
        import os
        from pathlib import Path
        with (Path(__file__).parent / 'message-calls').open('a') as log:
            log.write(json.dumps({'args': sys.argv[1:], 'value': value, 'pid': os.getpid()}) + '\n')
        if request['op'] == 'task_message':
            sys.exit(1)
        query = request['query']
        c = json.loads((Path(__file__).parent / 'fixture.json').read_text())
        receipt = c.get('message_receipt')
        print(json.dumps(dict(abi='desktop-task-message-v1', request_id=query['request_id'],
                              scope=query['scope'], read_only=True, found=receipt is not None,
                              receipt=receipt)))
        sys.exit(0)
