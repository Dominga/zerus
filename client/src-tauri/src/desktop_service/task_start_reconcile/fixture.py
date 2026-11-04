# Private transport fixture. Original child evidence comes from the actual
# native cache; the fixed result DTO is synthetic, with no native task delivery.
import sys
if sys.argv[1:] == ['--help']:
    import json
    from pathlib import Path
    c = json.loads((Path(__file__).parent / 'fixture.json').read_text())
    if not c.get('task_no_abi'):
        print('Desktop task start reconcile ABI: desktop-task-start-reconcile-v1', flush=True)
elif sys.argv[1:3] in [['desktop', 'frontend-request'], ['desktop', 'request']]:
    import io
    line = sys.stdin.readline()
    if not line:
        sys.exit(2)
    sys.stdin = io.StringIO(line)
    import json
    value = json.loads(line)
    request = value.get('request', value)
    if request.get('op') in ['task_start_reconcile', 'task_start_reconcile_result']:
        import hashlib
        import os
        import socket
        import time
        import uuid
        from pathlib import Path
        root = Path(__file__).parent
        config = json.loads((root / 'fixture.json').read_text())
        with (root / 'task-calls').open('a') as log:
            log.write(json.dumps({'op': request['op'], 'pid': os.getpid(),
                                 'reconcile_id': request.get('intent', request.get('query', {})).get('reconcile_id')}) + '\n')
        def digest(v):
            return hashlib.sha256(json.dumps(v, separators=(',', ':'), ensure_ascii=False).encode()).hexdigest()
        if request['op'] == 'task_start_reconcile':
            intent = request['intent']
            with socket.socket(socket.AF_UNIX) as stream:
                stream.settimeout(2)
                stream.connect(str(root / 'native-cache.sock'))
                stream.sendall(('desktop-ui-request-v1 ' + value['attempt'] + '\n').encode())
                evidence = json.loads(stream.makefile('rb').readline(8193))['request']
            assert evidence['child_pid'] == os.getpid()
            assert evidence['request_id'] == intent['reconcile_id']
            assert evidence['ownership_mode'] == 'service'
            assert evidence['fingerprint'] == digest(request)
            binding, key = intent['binding'], intent['binding']['key']
            seed = bytearray(b'desktop-operation-key-v1\0')
            for part in ['task_start_reconcile', '1', key['configuration'], key['project'], key['task'], key['request_id']]:
                b = part.encode(); seed.extend(len(b).to_bytes(4, 'big')); seed.extend(b)
            ticket = dict(operation_id=hashlib.sha256(seed).hexdigest(), attempt_id=intent['reconcile_id'],
                          revision=1, admission_id=str(uuid.uuid4()), binding_sha256=digest(binding),
                          writer_epoch=str(uuid.uuid4()), service_epoch=intent['scope']['service_epoch'], lease=str(uuid.uuid4()))
            attempt = dict(binding=binding, ticket=ticket, phase='submitted', drained=True,
                           outcome=dict(state='submitted', receipt=dict(schema=1, id=key['request_id'],
                                                                       project=key['project'], uuid=key['task'], state='submitted')))
            if config.get('task_unknown'):
                attempt.update(phase='unknown', outcome=dict(state='unknown', reason='lost_result'))
            (root / 'task-attempt').write_text(json.dumps(attempt))
            if config.get('task_hold'):
                (root / 'task-held').touch()
                if os.fork() == 0:
                    until = time.monotonic() + 12
                    while not (root / 'task-release').exists() and time.monotonic() < until:
                        time.sleep(.005)
                    os._exit(0)
                sys.exit(1)
            if config.get('task_lost'):
                sys.exit(1)
            print(json.dumps(dict(accepted=True, durable=True)))
        else:
            q = request['query']
            attempt = None if config.get('task_missing') else json.loads((root / 'task-attempt').read_text())
            print(json.dumps(dict(abi='desktop-task-start-result-v1', request_id=q['request_id'],
                                 scope=q['scope'], read_only=True, found=attempt is not None, attempt=attempt)))
        sys.exit(0)
