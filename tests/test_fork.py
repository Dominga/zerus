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

    def test_provider_returning_source_id_is_not_confirmed_as_a_fork(self):
        source=self.start('claude');(self.root/'broken-fork').touch()
        self.hgs('fork',source,'-n','broken','-d')
        self.wait(lambda:self.binding('claude/p/broken').get('error'))
        record=self.binding('claude/p/broken')
        self.assertIn('source conversation',record['error'])
        self.assertFalse(record.get('conversation_id'))

if __name__=='__main__': unittest.main()
