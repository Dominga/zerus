# Private transport prefix for the deferred answer client; the remainder is the
# existing service fixture. It records every deferred answer request and answers
# from fixture.json: `answer_admission` is the admission reply to print (absent:
# the reply is lost) and `answer_receipts` the lookup receipts, one per lookup
# with the last repeating (absent: none is found). As the real service does,
# the generic `desktop request` lane refuses the original itself.
import sys
if sys.argv[1:] == ['--help']:
    import json
    from pathlib import Path
    c = json.loads((Path(__file__).parent / 'fixture.json').read_text())
    if not c.get('answer_no_abi'):
        print('Desktop deferred answer ABI: desktop-deferred-answer-v1', flush=True)
elif sys.argv[1:3] in [['desktop', 'frontend-request'], ['desktop', 'request']]:
    import io
    import json
    line = sys.stdin.readline()
    sys.stdin = io.StringIO(line)
    value = json.loads(line)
    request = value.get('request', value)
    if request.get('op') in ['deferred_answer', 'deferred_answer_result']:
        import os
        from pathlib import Path
        root = Path(__file__).parent
        with (root / 'answer-calls').open('a') as log:
            log.write(json.dumps({'args': sys.argv[1:], 'value': value, 'pid': os.getpid()}) + '\n')
        c = json.loads((root / 'fixture.json').read_text())
        if request['op'] == 'deferred_answer' and sys.argv[2] != 'frontend-request':
            print(json.dumps(dict(request_id=None, accepted=False, durable=False,
                                  code='frontend_authentication_required')))
            sys.exit(0)
        if request['op'] == 'deferred_answer':
            admission = c.get('answer_admission')
            if admission is None:
                sys.exit(1)
            print(json.dumps(admission))
            sys.exit(0)
        query = request['query']
        receipts = c.get('answer_receipts') or []
        lookups = root / 'answer-lookups'
        seen = int(lookups.read_text()) if lookups.exists() else 0
        lookups.write_text(str(seen + 1))
        receipt = receipts[min(seen, len(receipts) - 1)] if receipts else None
        print(json.dumps(dict(abi='desktop-deferred-answer-v1', request_id=query['request_id'],
                              scope=query['scope'], read_only=True, found=receipt is not None,
                              receipt=receipt)))
        sys.exit(0)
