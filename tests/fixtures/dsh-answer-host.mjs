// Only an in-memory stand-in host loads the bridge; no native installation.
import assert from 'node:assert/strict';
import net from 'node:net';
import { join } from 'node:path';
const [bridge, cwd] = process.argv.slice(2);
const { apply } = await import(bridge);
const socket = join(cwd, 'host.sock'), listeners = {}, effects = [], agents = new Map();
const sessionId = 'session-00000000-0000-4000-8000-000000000001';
const requestId = '00000000-0000-4000-8000-000000000002';
const ctx = {
  sessionController: { agents: {
    composeAgent: async () => ({ agentPreset: 'standard', setup() {} }),
    presetForObservation() {}, serializeImageAdmission() {},
  } },
  agents: {
    get: id => agents.get(id), roots: () => [...agents.values()],
    create: async ({ sessionId }) => {
      const agent = { session: { id: sessionId, header: { origin: 'user', cwd } } };
      agents.set(sessionId, agent); return { agent, dispose() {} };
    },
  },
  agentDefaultModel: { currentSelection: () => ({ provider: 'fixture', model: 'fixture' }) },
  sessionQuery: { observeSession: async id => {
    if (!agents.has(id)) throw Object.assign(new Error('fixture absent'), { code: 'SESSION_QUERY_SESSION_NOT_FOUND' });
    return { header: agents.get(id).session.header, [Symbol.dispose]() {} };
  } },
  workspaceRegistry: { create: async () => ({ id: 'fixture', path: cwd, sessionIds: new Set(), attachSession() {} }) },
  get: () => undefined,
  on: (name, listener) => { (listeners[name] ??= []).push(listener); return () => {}; },
  effect: run => { effects.push(run()); },
};
apply(ctx, { socket });
await Promise.all(effects);
const call = (method, params = {}) => new Promise((resolve, reject) => {
  const client = net.connect(socket);
  let data = '';
  client.on('data', chunk => { data += chunk; });
  client.on('end', () => resolve(JSON.parse(data)));
  client.on('error', reject);
  client.write(JSON.stringify({ method, params }) + '\n');
});
assert.equal((await call('create', { sessionId, cwd })).ok, true);
const { value: { generation } } = await call('ping');
for (const legacy of [false, true]) {
  const signal = new AbortController().signal;
  const question = { id: 'choice', options: [{ label: 'One' }] };
  let settlements = 0;
  const pending = listeners['user-questions/request'][0]({ agent: agents.get(sessionId), signal,
    wait: { callId: 'question' }, questions: [question] }, () => { throw Error('question was not claimed'); });
  pending.then(() => { settlements++; });
  const params = { id: 'question', sessionId, generation,
    answers: [{ id: 'choice', selected: ['One'], custom: '' }], ...(legacy ? {} : { requestId }) };
  if (!legacy) {
    assert.equal((await call('answer', { ...params, requestId: 'not-a-uuid' })).ok, false);
    assert.equal(settlements, 0, 'invalid UUID resolved a question');
  }
  const stale = await call('answer', { ...params, generation: 'stale' });
  assert.equal(stale.ok, false); assert.equal(settlements, 0);
  const answered = await call('answer', params);
  assert.deepEqual(answered, { ok: true, value: legacy ? { accepted: true } : { accepted: true, requestId } });
  assert.deepEqual(await pending, { answers: params.answers });
  assert.equal(settlements, 1);
  assert.equal((await call('answer', params)).ok, false, 'consumed question cannot resolve twice');
  assert.equal(settlements, 1);
}
process.exit(0);
