"""Adapter controls; the native suites separately exercise the real engine."""
from pathlib import Path
import shutil
import subprocess
import unittest

BRIDGE = Path(__file__).resolve().parents[1] / 'src/dsh/bridge.mjs'


@unittest.skipUnless(shutil.which('node'), 'Node.js required')
class ModelSelection(unittest.TestCase):
    def run_js(self, body):
        script = """
import assert from 'node:assert/strict';
const { modelSelectionAdapter: adapt, completedForkCut, matchesForkCut,
        scheduleSession, manageSchedule } = await import(process.argv[2]);
const route = { provider: 'fixture', model: 'selected', reasoningEffort: 'off' };
const control = () => ({
  composeAgent: async () => ({ setup() {}, agentPreset: 'standard' }),
  presetForObservation() {}, serializeImageAdmission() {},
  selectionFor() { throw Error('read installed a selection'); },
  selectModel() { throw Error('profile default changed'); },
});
""" + body
        result = subprocess.run(['node', '--input-type=module', '-', BRIDGE.as_uri()],
                                input=script, text=True, capture_output=True, timeout=10)
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_legacy_reads_never_install_selection_and_keep_native_composition(self):
        self.run_js("""
const c = control(), composition = { setup() {}, agentPreset: 'original' };
c.composeAgent = async preset => { assert.equal(preset, 'chosen'); return composition; };
c.selections = new WeakMap();
const writes = [];
c.selectForNextRequest = (agent, value) => writes.push([agent, value]);
const m = adapt(c), agent = { options: { provider: 'fixture', model: 'inherited' } };
assert.equal(await m.compose('chosen'), composition);
assert.equal(m.has(agent), false);
assert.deepEqual(m.current(agent), agent.options);
assert.deepEqual(m.current(agent, { modelSelection: { next: route } }), route);
assert.equal(c.selections.has(agent), false);
c.selections.set(agent, { current: route });
assert.equal(m.has(agent), true);
const read = m.current(agent); read.model = 'changed copy';
assert.deepEqual(m.current(agent), route);
m.select(agent, route);
assert.deepEqual(writes, [[agent, route]]);
assert.deepEqual(agent.options, { provider: 'fixture', model: 'inherited' });
""")

    def test_agent_model_reads_and_writes_stay_on_that_original_agent(self):
        self.run_js("""
const c = control(), m = adapt(c), writes = [];
const agent = { options: { provider: 'fixture', model: 'inherited' }, model: {
  current() { assert.equal(this, agent.model); return route; },
  select(value) { assert.equal(this, agent.model); writes.push(value); },
} };
assert.equal(m.has(agent), true);
const read = m.current(agent, { modelSelection: { next: { model: 'stale' } } });
read.model = 'changed copy';
assert.deepEqual(m.current(agent), route);
assert.deepEqual(writes, []);
m.select(agent, route);
assert.deepEqual(writes, [route]);
assert.equal(agent.options.model, 'inherited');
assert.equal(m.current({ model: { current() {}, select() {} } }), null);
""")

    def test_new_model_is_checked_before_native_setup_and_publication(self):
        self.run_js("""
const c = control(), context = {}, calls = [];
const agent = { model: { current() {}, select() {} } };
const composition = { agentPreset: 'chosen', setup(ctx, original) {
  assert.equal(this, composition); calls.push([ctx, original]); return 'native setup';
} };
c.composeAgent = async () => composition;
const wrapped = await adapt(c).compose('chosen');
assert.equal(wrapped.agentPreset, 'chosen');
assert.throws(() => wrapped.setup(context, {}), /incompatible official/);
assert.deepEqual(calls, []);
assert.equal(wrapped.setup(context, agent), 'native setup');
assert.deepEqual(calls, [[context, agent]]);
""")

    def test_partial_controller_and_missing_image_serializer_refuse(self):
        self.run_js("""
for (const key of ['composeAgent', 'presetForObservation', 'serializeImageAdmission']) {
  const c = control(); delete c[key];
  assert.throws(() => adapt(c), /incompatible official/);
}
for (const partial of [
  { selections: new WeakMap() }, { selectForNextRequest() {} },
  { selections: new Map(), selectForNextRequest() {} },
]) assert.throws(() => adapt({ ...control(), ...partial }), /incompatible official/);
const c = control(); c.composeAgent = async () => ({});
await assert.rejects(adapt(c).compose('standard'), /incompatible official/);
""")

    def test_missing_agent_model_never_falls_back_to_stored_or_default_route(self):
        self.run_js("""
const m = adapt(control());
for (const model of [undefined, {}, { current() {} }, { select() {} }]) {
  const agent = { model, options: route };
  assert.throws(() => m.current(agent, { modelSelection: { next: route } }), /incompatible official/);
  assert.throws(() => m.select(agent, route), /incompatible official/);
  assert.throws(() => m.has(agent), /incompatible official/);
}
""")

    def test_new_fork_requires_the_exact_completed_source_prefix_and_seed_marker(self):
        self.run_js("""
const events = [
  { seq: 0, type: 'user/message', data: { content: 'original' } },
  { seq: 1, type: 'turn/end', data: {} },
  { seq: 2, type: 'user/message', data: { content: 'later unfinished turn' } },
];
const cut = completedForkCut({ events });
assert.equal(cut.atSeq, 1);
const original = { header: { isSeeded: true }, inheritedEventCount: 2,
  events: [...events.slice(0, 2), { seq: 2, type: 'session/end-seed', data: { inherited: true } }] };
assert.equal(matchesForkCut(original, cut), true);
for (const mutate of [
  x => x.header.isSeeded = false,
  x => x.header.parentSession = 'unrelated',
  x => x.inheritedEventCount = 1,
  x => x.events[0].data.content = 'another source',
  x => x.events[0].seq = 7,
  x => x.events[2].type = 'user/message',
  x => x.events[2].data.inherited = false,
  x => x.events[2].seq = 3,
]) {
  const changed = structuredClone(original); mutate(changed);
  assert.equal(matchesForkCut(changed, cut), false);
}
assert.throws(() => completedForkCut({ events: events.slice(0, 1) }), /completed reply/);
""")

    def test_new_schedules_bind_only_the_destination_and_preserve_native_cas(self):
        self.run_js("""
const entry = { id: 'task', kind: 'every', title: 'Before', prompt: 'Check',
  target: { kind: 'session', sessionId: 'original' }, involvedSessionIds: ['editor'],
  status: 'active', updatedAt: '2026-11-05T00:00:00.000Z' };
assert.equal(scheduleSession(entry), 'original');
assert.equal(scheduleSession({ sessionId: 'legacy' }), 'legacy');
assert.equal(scheduleSession({ ...entry, target: { kind: 'new' } }), undefined);
assert.equal(scheduleSession({ ...entry, sessionId: 'editor', target: null }), undefined);
const edits = [], histories = [];
const schedule = { async view({ id }) { assert.equal(id, 'task'); return entry; },
  async edit(request) { edits.push(request); return { ...entry, ...request.patch }; },
  async history(request) { histories.push(request); return { records: [
    { messageId: 'ours', targetSessionId: 'original' }, { messageId: 'foreign', targetSessionId: 'editor' },
  ], nextBefore: 'opaque' }; } };
const expected = { ...entry }; delete expected.status;
const p = { id: 'task', sessionId: 'original', action: 'update', expected, title: 'After' };
assert.equal((await manageSchedule(schedule, p)).updated, true);
assert.deepEqual(edits, [{ id: 'task', action: 'update', expectedUpdatedAt: entry.updatedAt, patch: { title: 'After' } }]);
const conflict = await manageSchedule(schedule, { ...p, expected: { ...expected, title: 'stale' } });
assert.equal(conflict.code, 'schedule_conflict'); assert.equal(edits.length, 1);
for (const action of ['history', 'delete', 'update']) {
  const result = await manageSchedule(schedule, { ...p, sessionId: 'editor', action, limit: 10 });
  assert.equal(result.code, 'schedule_not_found');
}
assert.deepEqual(histories, []); assert.equal(edits.length, 1);
const history = await manageSchedule(schedule, { ...p, action: 'history', limit: 10, before: 'cursor' });
assert.deepEqual(history, { records: [{ messageId: 'ours', targetSessionId: 'original' }], nextBefore: 'opaque' });
assert.deepEqual(histories, [{ id: 'task', limit: 10, before: 'cursor' }]);
// Retargeting inside the native queue after view must remain a CAS refusal.
schedule.edit = async request => {
  assert.equal(request.expectedUpdatedAt, entry.updatedAt);
  return { id: request.id, code: 'schedule_conflict' };
};
assert.equal((await manageSchedule(schedule, p)).updated, false);
assert.equal((await manageSchedule(schedule, { ...p, action: 'delete' })).code, 'schedule_conflict');
schedule.history = async () => {
  entry.target = { kind: 'session', sessionId: 'editor' };
  return { records: [{ messageId: 'foreign', targetSessionId: 'editor' }] };
};
assert.equal((await manageSchedule(schedule, { ...p, action: 'history', limit: 10 })).code, 'schedule_not_found');
""")

    def test_legacy_schedules_keep_the_original_session_scoped_methods(self):
        self.run_js("""
const calls = [], schedule = {};
for (const action of ['update', 'delete', 'history']) schedule[action] = async request => {
  calls.push([action, request]); return 'original result';
};
const p = { id: 'task', sessionId: 'original', expected: { id: 'task' }, title: 'After' };
assert.equal(await manageSchedule(schedule, { ...p, action: 'update' }), 'original result');
await manageSchedule(schedule, { ...p, action: 'delete' });
await manageSchedule(schedule, { ...p, action: 'history', limit: 10 });
assert.deepEqual(calls, [
  ['update', { sessionId: 'original', id: 'task', expected: p.expected, title: 'After' }],
  ['delete', { sessionId: 'original', id: 'task' }],
  ['history', { sessionId: 'original', id: 'task', limit: 10 }],
]);
await assert.rejects(manageSchedule({ update() {}, view() {}, edit() {} }, { ...p, action: 'delete' }), /incompatible official/);
""")


if __name__ == '__main__':
    unittest.main()
