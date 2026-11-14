"""Person-only attempt disposition cannot gain authority across a peer hop."""
import json,os,pathlib,subprocess,sys,tempfile,time,unittest,uuid
sys.path.insert(0,str(pathlib.Path(__file__).resolve().parents[1]))
from tests import test_tasks

# The stand-in SSH client crosses a real process-ancestry boundary before
# executing the same exact CLI on the private fixture's stand-in peer.
TRANSPORT=r"""#!/usr/bin/env python3
import json,os,pathlib,subprocess,sys,time,tempfile
root=pathlib.Path(os.environ['G1_TRANSPORT_ROOT'])
with tempfile.TemporaryDirectory(dir=root,prefix='peer-') as temp:
    call=pathlib.Path(temp)
    (call/'request').write_text(json.dumps({'args':sys.argv[1:],'input':sys.stdin.read(),'env':{k:v for k,v in os.environ.items() if not k.startswith(('HGS_SESSION','HGS_RUN_ID','HGS_AGENT','CLAUDE_'))}}))
    subprocess.run(['sh','-c','"$@" "$$" </dev/null >/dev/null 2>&1 &','g1-peer',sys.executable,str(root/'peer.py'),str(call)],check=True,timeout=5)
    deadline=time.monotonic()+45
    while not (call/'result').exists():
        if time.monotonic()>deadline: raise TimeoutError('peer did not settle')
        time.sleep(.005)
    reply=json.loads((call/'result').read_text());sys.stdout.write(reply['stdout']);sys.stderr.write(reply['stderr']);sys.exit(reply['code'])
"""
PEER=r"""import json,os,pathlib,subprocess,sys,time,shlex
call=pathlib.Path(sys.argv[1]);launcher=int(sys.argv[2]);deadline=time.monotonic()+5
while os.getppid()==launcher and time.monotonic()<deadline: time.sleep(.005)
assert os.getppid()!=launcher
q=json.loads((call/'request').read_text());env=q['env'];root=pathlib.Path(env['G1_TRANSPORT_ROOT'])
try:
    argv=q['args']
    if argv[-1]=='echo "hgs-remote-v2|%OS%|%SystemRoot%|$env:OS|$env:SystemRoot"':
        r={'code':0,'stdout':'hgs-remote-v2|%OS%|%SystemRoot%|:OS|:SystemRoot\n','stderr':''}
    else:
        args=shlex.split(argv[-1])[1:]
        with (root/'calls.jsonl').open('a') as out:out.write(json.dumps(args)+'\n')
        result=subprocess.run([env['G1_HGS'],*args],env=env,input=q['input'],text=True,capture_output=True,timeout=35)
        r={'code':result.returncode,'stdout':result.stdout,'stderr':result.stderr}
except Exception as e:r={'code':255,'stdout':'','stderr':repr(e)}
(call/'result.tmp').write_text(json.dumps(r));os.replace(call/'result.tmp',call/'result')
"""
STRIP=r"""import json,os,subprocess,sys,pathlib
request=json.loads(sys.argv[1]);env=dict(os.environ)
from tests.test_tasks import agent_record
agent_record(pathlib.Path(env['HGS_STATE_DIR']),env,env['HGS_SESSION'],env['HGS_RUN_ID'],pid=os.getpid())
for name in ('HGS_SESSION','HGS_RUN_ID','HGS_SESSION_ID'):env.pop(name,None)
r=subprocess.run(request['argv'],input=request.get('input',''),env=env,text=True,capture_output=True,timeout=55)
print(json.dumps({'code':r.returncode,'stdout':r.stdout,'stderr':r.stderr}))
"""

class TaskCancelAuthority(test_tasks.DeliveryFixture):
    start_plan = test_tasks.TaskDelivery.start_plan
    def unknown(self):
        warm=self.add('Warm private receipt folder','--target',self.name)
        self.hgs('start-deliver',warm['ref'],text=json.dumps(self.start_plan(warm)));self.confirm()
        task=self.add('Review unknown original','--target',self.name)
        plan=self.start_plan(task);receipts=self.state/'input_receipts';receipts.chmod(0o500)
        try: result=self.hgs('start-deliver',task['ref'],text=json.dumps(plan))
        finally:receipts.chmod(0o700)
        self.assertEqual(result['state'],'uncertain')
        return task,plan

    def test_late_reports_repeat_after_new_attempt_and_no_input(self):
        task,plan=self.unknown();received=self.received()
        cancelled=self.hgs('cancel-attempt',task['ref'],'--attempt',plan['id'])['task']
        late=json.loads(self.as_agent('report-attempt',task['ref'],'--attempt',plan['id'],'--json',text='Late original result'))
        self.assertEqual(late['task']['stage'],cancelled['stage'])
        self.assertEqual(late['task']['execution']['current']['phase'],'cancelled')
        self.assertFalse(late['moved_to_review'],late)
        self.assertEqual(self.received(),received,'abandonment and late report send no input')
        later=self.start_plan(task)
        delivered=self.hgs('start-deliver',task['ref'],text=json.dumps(later))
        self.assertEqual(delivered['state'],'submitted');self.wait_received(task['text'])
        before=self.hgs('get',task['ref'],'--history-limit','8')['task']
        again=self.hgs('cancel-attempt',task['ref'],'--attempt',plan['id'],'--reason','must not append again')
        self.assertTrue(again['attempt']['already_cancelled'])
        after=self.hgs('get',task['ref'],'--history-limit','8')['task']
        self.assertEqual(after,before,'exact repeat after a later attempt writes no fields or history')
        self.assertEqual(after['execution']['current']['id'],later['id'])
        late=json.loads(self.as_agent('report-attempt',task['ref'],'--attempt',plan['id'],'--json',text='Later old result'))
        current=self.hgs('get',task['ref'])['task']
        self.assertEqual(current['stage'],'active')
        self.assertEqual(current['execution']['current']['id'],later['id'])
        self.assertFalse(late['moved_to_review'],late)

    def test_stripped_readable_ancestor_cannot_cross_ssh_as_person(self):
        task,plan=self.unknown();args=['task','cancel-attempt',task['ref'],'--attempt',plan['id'],'--json']
        transport=self.root/'ssh';transport.write_text(TRANSPORT);transport.chmod(0o755)
        (self.root/'peer.py').write_text(PEER)
        env=dict(self.env,HGS_SSH=str(transport),G1_TRANSPORT_ROOT=str(self.root),G1_HGS=str(test_tasks.HGS))
        marked=dict(env,HGS_SESSION='codex/review-ancestor',HGS_RUN_ID=str(uuid.uuid4()))
        # Same readable marked ancestor, same stripped child, same exact action.
        def call(remote):
            argv=[str(test_tasks.HGS)]+(['@private-peer'] if remote else [])+args
            q=subprocess.run([sys.executable,'-c',STRIP,json.dumps({'argv':argv})],env=marked,text=True,capture_output=True,timeout=60)
            self.assertEqual(q.returncode,0,q.stderr)
            return json.loads(q.stdout)
        local=call(False);self.assertNotEqual(local['code'],0,local)
        self.assertIn('outside the agent',local['stderr'])
        remote=call(True)
        self.assertFalse((self.root/'calls.jsonl').exists(), 'the agent must stop before a peer command')
        self.assertIn('outside the agent',remote['stderr'])
        after=self.hgs('get',task['ref'])['task']
        self.assertIn(plan['id'],after['execution'].get('unknown',[]),'agent crossed SSH and released the unknown attempt as Person')
        self.assertNotEqual(remote['code'],0,remote)

if __name__=='__main__':unittest.main(verbosity=2)
