#!/usr/bin/env python3
"""Provider-native fork contracts with isolated tmux and scripted providers."""
import hashlib
import json
import os
from pathlib import Path
import unittest

import test_pause

test_pause.HGS = Path(os.environ.get('HGS_TEST_BIN', Path(__file__).resolve().parents[1]/'target/debug/hgs')).resolve()

AGENT = test_pause.AGENT.replace('with open(home / "argv.jsonl", "a") as f:', '''if agent == "kimi" and sys.argv[1:2] == ["fork"]:
    parent = sys.argv[2]
    sid = "session_" + str(uuid.uuid4())
    original = json.loads((home / "history" / (parent + ".jsonl")).read_text())
    original.update(session_id=sid, fork_parent=parent)
    (home / "history" / (sid + ".jsonl")).write_text(json.dumps(original) + "\\n")
    print("Forked to " + sid + " in 2ms")
    sys.exit(0)
with open(home / "argv.jsonl", "a") as f:''')
AGENT = AGENT.replace('if agent == "kimi":\n    hooks =', '''fork_parent = None
if agent == "codex" and "fork" in args:
    fork_parent = args[args.index("fork") + 1]
elif agent == "claude" and "--fork-session" in args:
    fork_parent = args[args.index("--resume") + 1]
if fork_parent:
    sid = fork_parent if (home / "broken-fork").exists() else str(uuid.uuid4())
existing = home / "history" / ((fork_parent or sid) + ".jsonl")
inherited = json.loads(existing.read_text()) if existing.exists() else {}
context = inherited.get("context", [])
fork_parent = fork_parent or inherited.get("fork_parent")
if agent == "kimi":
    hooks =''')
AGENT = AGENT.replace('path.write_text(json.dumps({"session_id": sid}) + "\\n")', 'path.write_text(json.dumps({"session_id": sid, "fork_parent": fork_parent, "context": context}) + "\\n")')

# Reuse only the fixture infrastructure, not the lifecycle test methods.
Base = type('ForkFixture', (unittest.TestCase,), {name:value for name,value in test_pause.Harness.__dict__.items()
    if name not in ('__dict__','__weakref__') and not name.startswith('test_')})

class Forks(Base):
    def setUp(self):
        original=test_pause.AGENT;test_pause.AGENT=AGENT
        try: super().setUp()
        finally: test_pause.AGENT=original

    def fork(self,name,tag='branch',*args):
        source=self.binding(name)
        self.hgs('fork',name,'-n',tag,'-d','--expected-run-id',source['run_id'],
                 '--expected-conversation-id',source['conversation_id'],*args)
        target='/'.join(name.split('/')[:2])+'/'+tag
        self.wait(lambda:self.binding(target).get('conversation_id'))
        return target

    def test_live_native_fork_preserves_original_and_context_for_all_providers(self):
        for agent in ['codex','claude','kimi']:
            source=self.start(agent)
            record=self.binding(source);path=self.root/'state'/(hashlib.sha256(source.encode()).hexdigest()+'.json')
            transcript=Path(record['transcript']);body=json.loads(transcript.read_text());body['context']=['user request','agent reply'];transcript.write_text(json.dumps(body)+'\n')
            original=path.read_bytes();history=transcript.read_bytes()
            target=self.fork(source)
            # zerus-ucck P1: a fork is another session with its own durable ID.
            self.assertNotEqual(self.binding(target)['session_id'],record.get('session_id'))
            import uuid;self.assertEqual(str(uuid.UUID(self.binding(target)['session_id'])),self.binding(target)['session_id'])
            forked=self.binding(target)
            self.assertNotEqual(forked['conversation_id'],record['conversation_id'])
            self.assertEqual(forked['fork_parent_id'],record['conversation_id'])
            self.assertEqual(forked['fork_source_name'],source)
            self.assertEqual(path.read_bytes(),original);self.assertEqual(transcript.read_bytes(),history)
            cloned=json.loads(Path(forked['transcript']).read_text())
            self.assertEqual(cloned['context'],['user request','agent reply'])
            self.assertEqual(forked['activity'],'idle')
            self.assertEqual(self.binding(source)['pid'],record['pid'])
            self.hgs('pause',target)
            self.resume(target)
            self.assertEqual(self.binding(target)['conversation_id'],forked['conversation_id'])
            self.assertEqual(self.binding(target)['fork_parent_id'],record['conversation_id'])
            self.assertEqual(path.read_bytes(),original);self.assertEqual(transcript.read_bytes(),history)

    def test_saved_and_archived_sources_can_fork_without_resuming_original(self):
        source,path=self.saved_v1('codex')
        before=path.read_bytes()
        branch=self.fork(source['name'],'saved-branch')
        self.assertEqual(path.read_bytes(),before)
        self.assertNotEqual(self.binding(branch)['conversation_id'],source['conversation_id'])
        self.hgs('archive',source['name'])
        archived=list((self.root/'state/archives').glob('*.json'))[0]
        record=json.loads(archived.read_text());before=archived.read_bytes()
        self.hgs('fork',source['name'],'-n','archive-branch','-d','--archive',record['archive_id'])
        self.wait(lambda:self.binding('codex/p/archive-branch').get('conversation_id'))
        self.assertEqual(archived.read_bytes(),before)
        self.assertFalse(path.exists())

    def test_dry_run_is_side_effect_free_and_guards_stale_busy_collisions(self):
        source=self.start('codex');record=self.binding(source)
        before=set((self.root/'state').glob('*.json'))
        output=self.hgs('fork',source,'-d','--dry-run')
        self.assertIn('codex fork '+record['conversation_id'],output)
        self.assertEqual(set((self.root/'state').glob('*.json')),before)
        self.hgs('fork',source,'-d','--expected-run-id','stale',rc=1)
        # A name in use is not a refusal: the dry run shows the free name.
        self.assertIn('codex/p/one-2',self.hgs('fork',source,'-n','one','-d','--dry-run'))
        self.send_event(source,'UserPromptSubmit')
        self.hgs('fork',source,'-d',rc=1)
        self.assertEqual(set((self.root/'state').glob('*.json')),before)

    def test_automatic_names_do_not_reuse_existing_branch(self):
        source=self.start('codex')
        self.hgs('fork',source,'-d');self.wait(lambda:self.binding('codex/p/one-fork').get('conversation_id'))
        self.hgs('fork',source,'-d');self.wait(lambda:self.binding('codex/p/one-fork-2').get('conversation_id'))
        self.assertNotEqual(self.binding('codex/p/one-fork')['conversation_id'],self.binding('codex/p/one-fork-2')['conversation_id'])

    # zerus-ucck: a session name is a label. An archived session never blocks a
    # fork or a new session from its name, and a name a live or saved session
    # uses gets the first free -2, -3... instead of failing.
    def test_archived_names_are_free_and_taken_names_get_a_suffix(self):
        source=self.start('codex')
        archived,_=self.saved_v1('codex',tag='one-fork')
        self.hgs('archive',archived['name'])
        self.hgs('fork',source,'-d');self.wait(lambda:self.binding('codex/p/one-fork').get('conversation_id'))
        self.hgs('fork',source,'-n','one-fork','-d')
        self.wait(lambda:self.binding('codex/p/one-fork-2').get('conversation_id'))
        self.assertEqual(len(list((self.root/'state/archives').glob('*.json'))),1)
        reused,_=self.saved_v1('claude',tag='reused')
        self.hgs('archive',reused['name'])
        self.assertEqual(self.binding(self.start('claude','reused'))['name'],'claude/p/reused')

    def swarm(self,*args,request=None):
        import subprocess
        p=subprocess.run([str(test_pause.HGS),'swarm',*args],input=None if request is None else json.dumps(request),
                         env=self.env,capture_output=True,text=True,timeout=20)
        self.assertEqual(p.returncode,0,p.stdout+p.stderr);return json.loads(p.stdout)

    def project_of(self,name,node):
        import uuid
        record=self.binding(name)
        return self.swarm('session-project','--json',request={'request_id':str(uuid.uuid4()),'name':name,
            'expected_run_id':record['run_id'],'expected_conversation_id':record['conversation_id'],
            'expected_machine_id':node})['current_project_id']

    # zerus-zjsj: a fork starts in its source session's project, live or
    # archived, written by the catalog owner when the fork's binding exists.
    def test_a_fork_starts_in_its_source_sessions_project(self):
        source=self.start('codex')
        saved,_=self.saved_v1('codex',tag='kept')
        self.hgs('archive',saved['name'])
        archive_id=json.loads(next((self.root/'state/archives').glob('*.json')).read_text())['archive_id']
        loose=self.start('claude','loose')
        node=self.swarm('hello')['node_id']
        self.swarm('initialize',request={'projects':[dict(id='work',name='Work',color='#123456',folders=[],
            sessions=[node+'\n'+source,node+'\narchive\n'+archive_id])]})
        self.assertEqual(self.project_of(source,node),'work')
        self.assertEqual(self.project_of(self.fork(source,'placed'),node),'work')
        self.hgs('fork',saved['name'],'-n','from-archive','-d','--archive',archive_id)
        self.wait(lambda:self.binding('codex/p/from-archive').get('conversation_id'))
        self.assertEqual(self.project_of('codex/p/from-archive',node),'work')
        # A source in the default project leaves its fork there too.
        self.assertEqual(self.project_of(self.fork(loose,'loose-fork'),node),'ungrouped')

    # zerus-wx3k: a fork waits for a busy catalog without holding the state
    # lock, then still writes its inherited project while its binding is the same.
    def test_a_fork_waits_for_a_busy_catalog_outside_the_state_lock(self):
        import fcntl,time
        source=self.start('codex')
        node=self.swarm('hello')['node_id']
        self.swarm('initialize',request={'projects':[dict(id='work',name='Work',color='#123456',folders=[],
            sessions=[node+'\n'+source])]})
        record=self.binding(source)
        target='/'.join(source.split('/')[:2])+'/busy'
        path=self.root/'state'/(hashlib.sha256(target.encode()).hexdigest()+'.json')
        with open(self.root/'cfg'/'swarm'/'catalog.lock','r+') as catalog:
            fcntl.flock(catalog,fcntl.LOCK_EX)
            try:
                self.hgs('fork',source,'-n','busy','-d','--expected-run-id',record['run_id'],
                         '--expected-conversation-id',record['conversation_id'])
                self.wait(path.exists)
                # The fork's launch now waits for the catalog; the state lock is free.
                with open(self.root/'state'/'.lock','a') as state:
                    until=time.monotonic()+1.5
                    while True:
                        try:
                            fcntl.flock(state,fcntl.LOCK_EX|fcntl.LOCK_NB);break
                        except BlockingIOError:
                            self.assertLess(time.monotonic(),until,'the state lock stayed held while the catalog was busy')
                            time.sleep(0.01)
                    fcntl.flock(state,fcntl.LOCK_UN)
            finally:
                fcntl.flock(catalog,fcntl.LOCK_UN)
        self.wait(lambda:self.binding(target).get('conversation_id'))
        self.assertEqual(self.project_of(target,node),'work')

    # zerus-b4qk: a fork's Activity shows its source's history up to the fork,
    # marked as inherited, then its own; the source's later rows stay out.
    def test_a_fork_shows_its_sources_history_up_to_the_fork(self):
        import time
        talk=lambda events:[(e['detail'],e.get('inherited_from')) for e in events
                            if e['type'] in ('UserPromptSubmit','Stop')]
        shown=lambda name:json.loads(self.hgs('inspect',name))
        for agent in ['codex','claude']:
            source=self.start(agent,f'{agent}-root')
            self.send_event(source,'UserPromptSubmit',prompt='source request')
            self.send_event(source,'Stop',last_assistant_message='source reply')
            time.sleep(1.1)
            fork=self.fork(source,f'{agent}-branch')
            # Resume carries the labels with the lineage (#1408 review).
            labels=self.binding(fork)['fork_lineage_labels']
            self.assertEqual(labels,{self.binding(source)['journal_name']:source})
            self.hgs('pause',fork);self.resume(fork)
            self.assertEqual(self.binding(fork)['fork_lineage_labels'],labels)
            self.send_event(source,'UserPromptSubmit',prompt='source after the fork')
            self.send_event(fork,'UserPromptSubmit',prompt='fork request')
            inherited=[('source request',source),('source reply',source)]
            data=shown(fork)
            self.assertEqual(talk(data['events']),inherited+[('fork request',None)],agent)
            self.assertEqual(talk(data['message_events']),inherited+[('fork request',None)],agent)
            # The lineage names the source's journal (its ID journal since zerus-ucck P2b);
            # the rows above show the label it had.
            [[journal,conversation,seq]]=self.binding(fork)['fork_lineage']
            source_journal=self.binding(source)['journal_name']
            self.assertEqual((journal,conversation),(source_journal,self.binding(source)['conversation_id']))
            # The source itself is unchanged and inherits nothing.
            self.assertEqual(talk(shown(source)['events']),[('source request',None),('source reply',None),('source after the fork',None)])
            # A fork of the fork reaches back through both.
            self.send_event(fork,'Stop',last_assistant_message='fork reply')
            time.sleep(1.1)
            second=self.fork(fork,f'{agent}-twig')
            self.send_event(second,'UserPromptSubmit',prompt='twig request')
            self.assertEqual(talk(shown(second)['events']),
                inherited+[('fork request',fork),('fork reply',fork),('twig request',None)],agent)
            # A fork recorded before the lineage: its source's rows up to its creation.
            path=self.root/'state'/(hashlib.sha256(fork.encode()).hexdigest()+'.json')
            record=json.loads(path.read_text());record.pop('fork_lineage');path.write_text(json.dumps(record))
            self.assertEqual(talk(shown(fork)['events']),inherited+[('fork request',None),('fork reply',None)],agent)
            # Found once and saved under the state lock, so later reads do not search.
            [[derived_journal,derived_conversation,derived]]=self.binding(fork)['fork_lineage']
            self.assertEqual((derived_journal,derived_conversation),(source_journal,conversation))
            self.assertLessEqual(derived,seq)
            self.assertEqual(talk(shown(fork)['events']),inherited+[('fork request',None),('fork reply',None)],agent)
            self.assertEqual(self.binding(fork)['fork_lineage'],[[source_journal,conversation,derived]])
            # Search finds the source's words in the source, never again in its forks.
            hits=json.loads(self.hgs('search','--query','source request'))['results']
            self.assertIn(source,{h['name'] for h in hits},agent)
            self.assertFalse({fork,second}&{h['name'] for h in hits},agent)

    # zerus-b4qk.1: the phone history (`hgs history`) of a fork shows its
    # source's rows up to the fork, marked and never counted as unread.
    def test_a_forks_phone_history_shows_its_source_up_to_the_fork(self):
        import time,subprocess,uuid
        def history(name):
            record=self.binding(name)
            for _ in range(12):
                p=subprocess.run([str(test_pause.HGS),'history',name,'--json'],env=self.env,capture_output=True,text=True,timeout=20,
                    input=json.dumps(dict(request_id=str(uuid.uuid4()),expected_run_id=record['run_id'],
                                          expected_conversation_id=record['conversation_id'],limit=100)))
                self.assertEqual(p.returncode,0,p.stdout+p.stderr)
                page=json.loads(p.stdout)
                if page['head']['complete']:
                    return page
            self.fail('history did not finish indexing: '+p.stdout)
        talk=lambda page:[(e['detail'],e.get('inherited_from')) for e in page['events']]
        source=self.start('claude','phone-root')
        kinds=lambda page:[(e['type'],e.get('inherited_type')) for e in page['events'] if e.get('inherited_from')]
        self.send_event(source,'UserPromptSubmit',prompt='source request')
        self.send_event(source,'Stop',last_assistant_message='source reply')
        time.sleep(1.1)
        fork=self.fork(source,'phone-branch')
        self.send_event(source,'UserPromptSubmit',prompt='source after the fork')
        self.send_event(fork,'UserPromptSubmit',prompt='fork request')
        before=history(fork)
        self.send_event(fork,'Stop',last_assistant_message='fork reply')
        page=history(fork)
        self.assertEqual(talk(page),[('source request',source),('source reply',source),('fork request',None),('fork reply',None)])
        # Their own type, so an app that does not know it drops them, never
        # counting a source reply as unread; the original type stays.
        self.assertEqual(kinds(page),[('Inherited','UserPromptSubmit'),('Inherited','Stop')])
        # Inherited replies are never unread in the fork: only its own reply counts.
        self.assertEqual(page['head']['total_incoming'],1)
        self.assertEqual([e['incoming_seq'] for e in page['events'] if e['detail']=='source reply'],[None])
        self.assertEqual(page['history_epoch'],before['history_epoch'])
        # A new fork's first read is no backfill (#1248 N3).
        self.assertNotIn('canonical_order_reset',before['source_status'])
        # The source's own history is unchanged and inherits nothing.
        self.assertEqual(talk(history(source)),[('source request',None),('source reply',None),('source after the fork',None)])
        # A copy of the source history in the fork's own transcript (dated
        # before the fork) is not shown again; the fork's own later line is.
        transcript=Path(self.binding(fork)['transcript'])
        line=lambda uuid_,at,text:json.dumps({"type":"assistant","uuid":uuid_,"timestamp":at,"sessionId":self.binding(fork)['conversation_id'],
            "message":{"role":"assistant","content":[{"type":"text","text":text}]}})+"\n"
        with transcript.open('a') as out:
            out.write(line('copied','2001-01-01T00:00:00Z','copied source line'))
            out.write(line('later',time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime(time.time()+5)),'later own line'))
        details=[d for d,_ in talk(history(fork))]
        self.assertNotIn('copied source line',details)
        self.assertIn('later own line',details)
        # An index that existed before its fork had a lineage gains the source
        # rows later, with a new epoch so a reader that saw the oldest row reads again.
        self.send_event(source,'Stop',last_assistant_message='source later reply')
        late=self.fork(source,'phone-late')
        path=self.root/'state'/(hashlib.sha256(late.encode()).hexdigest()+'.json')
        record=json.loads(path.read_text());kept={k:record.pop(k) for k in ('fork_lineage','fork_source_name','fork_parent_id')}
        path.write_text(json.dumps(record))
        self.send_event(late,'UserPromptSubmit',prompt='late request')
        # Indexed as an older CLI did, with a provider copy of the source history.
        late_transcript=Path(self.binding(late)['transcript'])
        with late_transcript.open('a') as out:
            for uuid_,at,text in (('late-copy','2001-01-01T00:00:00Z','late copied line'),
                                  ('late-own',time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime(time.time()+5)),'late own line')):
                out.write(json.dumps({"type":"assistant","uuid":uuid_,"timestamp":at,"sessionId":self.binding(late)['conversation_id'],
                    "message":{"role":"assistant","content":[{"type":"text","text":text}]}})+"\n")
        first=history(late)
        self.assertEqual(talk(first),[('late copied line',None),('late request',None),('late own line',None)])
        self.assertEqual(first['head']['total_incoming'],2)
        record=json.loads(path.read_text());record.update(kept);path.write_text(json.dumps(record))
        second=history(late)
        # Only the copy dated before the fork goes; the fork's own later line
        # stays, and incoming rows are numbered again (#1248 N8).
        self.assertEqual(talk(second),[('source request',source),('source reply',source),('source after the fork',source),
                                       ('source later reply',source),('late request',None),('late own line',None)])
        self.assertEqual(second['head']['total_incoming'],1)
        self.assertNotEqual(second['history_epoch'],first['history_epoch'])
        self.assertEqual(second['source_status']['canonical_order_reset'],'inherited_backfill')
        # A long source loads over several bounded calls, reported as indexing.
        import sqlite3
        big=self.start('claude','phone-big')
        self.send_event(big,'UserPromptSubmit',prompt='big request')
        self.send_event(big,'Stop',last_assistant_message='big reply')
        record=self.binding(big)
        with sqlite3.connect(self.root/'state'/'events.sqlite3') as db:
            db.executemany("INSERT INTO events(name,conversation,payload) VALUES(?,?,?)",
                [(record.get('journal_name') or big,record['conversation_id'],
                  json.dumps({"type":"Stop","agent_id":"","at":time.time()-5,"detail":f"bulk {i}"})) for i in range(2500)])
        time.sleep(1.1)
        branch=self.fork(big,'phone-big-branch')
        self.send_event(branch,'UserPromptSubmit',prompt='branch request')
        branch_record=self.binding(branch)
        once=subprocess.run([str(test_pause.HGS),'history',branch,'--json'],env=self.env,capture_output=True,text=True,timeout=20,
            input=json.dumps(dict(request_id=str(uuid.uuid4()),expected_run_id=branch_record['run_id'],
                                  expected_conversation_id=branch_record['conversation_id'],limit=100)))
        once=json.loads(once.stdout)
        self.assertEqual((once['head']['complete'],once['indexing'],once['source_status']['inherited']),(False,True,'indexing'))
        done=history(branch)
        self.assertEqual(done['source_status']['inherited'],'complete')
        # Rows sort by time: the bulk rows were dated before the source's own exchange.
        self.assertEqual(talk(done)[-3:],[('big request',big),('big reply',big),('branch request',None)])
        self.assertIn(('bulk 2499',big),talk(done))
        self.assertEqual(done['head']['total_incoming'],0)
        # A fork recorded before its lineage finds its source once and saves it (#1248 N7).
        path=self.root/'state'/(hashlib.sha256(fork.encode()).hexdigest()+'.json')
        record=json.loads(path.read_text());record.pop('fork_lineage');path.write_text(json.dumps(record))
        history(fork)
        [[saved_source,saved_conversation,_]]=self.binding(fork)['fork_lineage']
        self.assertEqual((saved_source,saved_conversation),(self.binding(source)['journal_name'],self.binding(source)['conversation_id']))

    def test_provider_returning_source_id_is_not_confirmed_as_a_fork(self):
        source=self.start('claude');(self.root/'broken-fork').touch()
        self.hgs('fork',source,'-n','broken','-d')
        self.wait(lambda:self.binding('claude/p/broken').get('error'))
        record=self.binding('claude/p/broken')
        self.assertIn('source conversation',record['error'])
        self.assertFalse(record.get('conversation_id'))

if __name__=='__main__': unittest.main()
