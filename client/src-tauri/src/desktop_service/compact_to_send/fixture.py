# Private transport prefix for the compact-to-send client; the remainder is
# the existing service fixture. It records every compact-to-send request and
# answers from fixture.json: `compact_admission` is the admission reply to
# print (absent: the reply is lost) and `compact_receipts` the lookup
# receipts, one per lookup with the last repeating (absent: none is found).
# As the real service does, the generic `desktop request` lane refuses the
# original itself.
import sys
if sys.argv[1:] == ['--help']:
    import json
    from pathlib import Path
    c = json.loads((Path(__file__).parent / 'fixture.json').read_text())
    if not c.get('compact_no_abi'):
        print('Desktop deferred compact to send ABI: desktop-deferred-compact-to-send-v1', flush=True)
elif sys.argv[1:3] in [['desktop', 'frontend-request'], ['desktop', 'request']]:
    import io
    import json
    line = sys.stdin.readline()
    sys.stdin = io.StringIO(line)
    value = json.loads(line)
    request = value.get('request', value)
    if request.get('op') in ['compact_to_send', 'compact_to_send_result']:
        import os
        from pathlib import Path
        root = Path(__file__).parent
        with (root / 'compact-calls').open('a') as log:
            log.write(json.dumps({'args': sys.argv[1:], 'value': value, 'pid': os.getpid()}) + '\n')
        c = json.loads((root / 'fixture.json').read_text())
        if request['op'] == 'compact_to_send' and sys.argv[2] != 'frontend-request':
            print(json.dumps(dict(request_id=None, accepted=False, durable=False,
                                  code='frontend_authentication_required')))
            sys.exit(0)
        if request['op'] == 'compact_to_send':
            admission = c.get('compact_admission')
            if admission is None:
                sys.exit(1)
            print(json.dumps(admission))
            sys.exit(0)
        query = request['query']
        receipts = c.get('compact_receipts') or []
        lookups = root / 'compact-lookups'
        seen = int(lookups.read_text()) if lookups.exists() else 0
        lookups.write_text(str(seen + 1))
        receipt = receipts[min(seen, len(receipts) - 1)] if receipts else None
        print(json.dumps(dict(abi='desktop-deferred-compact-to-send-v1', request_id=query['request_id'],
                              scope=query['scope'], read_only=True, found=receipt is not None,
                              receipt=receipt)))
        sys.exit(0)
