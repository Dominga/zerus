"""Closed proof across the retained Python connector and legacy API v1."""
import asyncio
import copy
import json
from pathlib import Path
import tempfile
import unittest
from zerus_mobile import send_outcome as c
from zerus_mobile.connector import Connector, ConnectorError
from zerus_mobile.server import validate_request

VECTORS=json.loads((Path(__file__).resolve().parents[1]/'vectors/send_outcome/outcomes.json').read_text())['vectors']

class OutcomeContract(unittest.TestCase):
    def test_shared_binding_and_native_proof_vectors(self):
        for v in VECTORS:
            for op in ('send','read'):
                r=v[op];validate_request(r);c.validate(r['operation'],r['payload'],r['request_id'],r['computer_id'],r['session'])
            self.assertEqual(c.project(v['send']['payload'],v['negative'],True),v['not_sent'])
            for native,result in [('read_negative','read_not_sent'),('read_positive','read_submitted'),('read_unknown','unknown')]:
                self.assertEqual(c.project(v['read']['payload'],v[native]),v[result])
    def test_no_foreign_partial_retryable_or_corrupt_negative_is_proof(self):
        for v in VECTORS:
            for field in ['request_id','name','machine_id','session_id','provider','run_id','conversation_id','native_digest','proof','schema']:
                with self.subTest(field=field):
                    n=copy.deepcopy(v['negative']);n[field]='changed'
                    with self.assertRaises((ValueError,TypeError)):c.project(v['send']['payload'],n,True)
            for change in [dict(status='refused_before_input'),dict(proof='stderr'),dict(extra='unreviewed')]:
                with self.assertRaises(ValueError):c.project(v['send']['payload'],dict(v['negative'],**change),True)
            bad=copy.deepcopy(v['send']);bad['payload']['payload']['text']+='changed'
            with self.assertRaises(ValueError):validate_request(bad)

class ConnectorJournal(unittest.IsolatedAsyncioTestCase):
    async def test_duplicate_native_fields_never_form_a_not_sent_proof(self):
        vector=VECTORS[0];packet=json.dumps(vector['negative'])
        with tempfile.TemporaryDirectory(prefix='zerus-outcome-duplicate-') as temp:
            root=Path(temp);hgs=root/'hgs'
            hgs.write_text('#!/usr/bin/env python3\nimport sys\nsys.stdin.read()\nprint('+repr('{"schema":2,'+packet[1:])+')\nsys.exit(1)\n');hgs.chmod(0o700)
            n=Connector(dict(server_url='https://relay.example.invalid',node_token='fixture-token',hgs_path=str(hgs),state_dir=str(root/'journal')))
            try:
                with self.assertRaises(ConnectorError):
                    await n._native(['send',vector['send']['session'],'--delivery-outcome-v2','--json'],vector['send']['payload']['payload'])
            finally:n.journal.close()

    async def test_only_exact_negative_settles_and_no_duplicate_calls_native(self):
        for variant in ['negative','foreign','timeout','unknown','positive']:
            with tempfile.TemporaryDirectory(prefix='zerus-outcome-') as temp:
                n=Connector(dict(server_url='https://relay.example.invalid',node_token='fixture-token',hgs_path='fixture-hgs',state_dir=temp))
                n.send_outcome_abi=True
                v=copy.deepcopy(next(v for v in VECTORS if v['name']=='python_default-attachment'))
                n.sessions={v['send']['session']};calls=[]
                async def native(argv,payload=None,**kwargs):
                    calls.append((argv,payload))
                    if variant=='timeout':raise ConnectorError('native command timed out')
                    if argv[0]=='__state':
                        answer=dict(v['read_positive' if variant=='positive' else 'read_unknown'],request_id=payload['request_id'])
                        return answer
                    answer=copy.deepcopy(v['negative'])
                    if variant=='foreign':answer['native_digest']='a'*64
                    if variant in ('unknown','positive'):answer['status']='submitted'
                    return answer
                n.native=native
                class Fleet:
                    async def resolve(self,*args,**kwargs):return n,None
                n.fleet=Fleet()
                try:
                    first=await n.execute(v['send'])
                    self.assertEqual(first['state'],{'negative':'failed','positive':'completed'}.get(variant,'uncertain'),first)
                    if variant=='negative':self.assertEqual(first['result'],v['not_sent'])
                    # execute() claims are checked independently below; a recovery
                    # read must preserve the original mutation's exact response.
                    before=n.journal.pending()
                    count=len(calls);await n.execute(v['send']);self.assertEqual(len(calls),count)
                    if variant=='negative':
                        async def read(argv,payload=None,**kwargs):
                            self.assertEqual(argv[0],'__state');calls.append((argv,payload));return v['read_negative']
                        n.native=read
                        result=await n.execute(v['read']);self.assertEqual(result['result'],v['read_not_sent'])
                        self.assertEqual(n.journal.pending()[0],before[0])
                finally:n.journal.close()

if __name__=='__main__':unittest.main()
