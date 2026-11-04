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
elif sys.argv[1:3] in [['desktop', 'frontend-request'], ['desktop', 'request']]:
    import io
    import json
    line = sys.stdin.readline()
    sys.stdin = io.StringIO(line)
    value = json.loads(line)
    request = value.get('request', value)
    if request.get('op') in ['task_message', 'task_message_result']:
        import os
        from pathlib import Path
        with (Path(__file__).parent / 'message-calls').open('a') as log:
            log.write(json.dumps({'args': sys.argv[1:], 'value': value, 'pid': os.getpid()}) + '\n')
        if request['op'] == 'task_message':
            sys.exit(1)
        query = request['query']
        print(json.dumps(dict(abi='desktop-task-message-v1', request_id=query['request_id'],
                              scope=query['scope'], read_only=True, found=False, receipt=None)))
        sys.exit(0)
