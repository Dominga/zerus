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
            self.send_event(source,'UserPromptSubmit',prompt='source after the fork')
            self.send_event(fork,'UserPromptSubmit',prompt='fork request')
            inherited=[('source request',source),('source reply',source)]
            data=shown(fork)
            self.assertEqual(talk(data['events']),inherited+[('fork request',None)],agent)
            self.assertEqual(talk(data['message_events']),inherited+[('fork request',None)],agent)
            [[journal,conversation,seq]]=self.binding(fork)['fork_lineage']
            self.assertEqual((journal,conversation),(source,self.binding(source)['conversation_id']))
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
            # Search finds the source's words in the source, never again in its forks.
            hits=json.loads(self.hgs('search','--query','source request'))['results']
            self.assertIn(source,{h['name'] for h in hits},agent)
            self.assertFalse({fork,second}&{h['name'] for h in hits},agent)

    def test_provider_returning_source_id_is_not_confirmed_as_a_fork(self):
        source=self.start('claude');(self.root/'broken-fork').touch()
        self.hgs('fork',source,'-n','broken','-d')
        self.wait(lambda:self.binding('claude/p/broken').get('error'))
        record=self.binding('claude/p/broken')
        self.assertIn('source conversation',record['error'])
        self.assertFalse(record.get('conversation_id'))

if __name__=='__main__': unittest.main()
