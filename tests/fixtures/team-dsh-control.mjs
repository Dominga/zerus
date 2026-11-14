// Faults use the reviewed engine's own inbox/step writers. Loaded only by
// team-dsh-host.py in its private keyless fixture, never by a product host.
import { existsSync, readFileSync, appendFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { connect } from 'node:net';
export const inject = ['sessionController', 'agents'];
export function apply(ctx, config) {
  writeFileSync(join(config.root, 'native-control-ready'), 'ready');
  const mode = () => existsSync(join(config.root, 'native-mode')) ? readFileSync(join(config.root, 'native-mode'), 'utf8').trim() : '';
  const rpc = (method, params) => new Promise((resolve, reject) => {
    const socket = connect(config.socket);
    let text = '';
    socket.setTimeout(15000, () => socket.destroy(new Error('Fixture RPC timed out')));
    socket.on('connect', () => socket.write(JSON.stringify({ method, params }) + '\n'));
    socket.on('error', reject);
    socket.on('data', data => {
      text += data;
      if (!text.includes('\n')) return;
      const reply = JSON.parse(text.split('\n')[0]); socket.end();
      if (reply.ok) resolve(reply.value); else reject(new Error(reply.error));
    });
  });
  const api = ctx.sessionController, prompt = api.prompt.bind(api);
  api.prompt = async (...args) => {
    appendFileSync(join(config.root, 'native-offers'), 'offer\n');
    if (mode() === 'host_refused') throw new Error('The fixture host refused before inserting anything');
    if (mode() === 'unseen') return {};
    const answer = await prompt(...args);
    if (mode() === 'host_disposed') {
      const ping = await rpc('ping', {});
      const gone = await rpc('session-action', {sessionId:args[0].sessionId, action:'forget', generation:ping.generation});
      if (gone.status !== 'completed') throw new Error('Fixture disposal did not settle');
    }
    return answer;
  };
  ctx.on('agent/inbox/inserted', ({ agent, message }) => {
    appendFileSync(join(config.root, 'native-inserts'), JSON.stringify({rpcId:message.source?.rpcId}) + '\n');
    if (mode() === 'canceled') agent.inbox.remove(message.id);
  });
  ctx.on('agent/pre-step', async (payload, next) => {
    if (mode() === 'dropped_after_claim') throw new Error('The fixture rejects the claimed step');
    if (mode() === 'pending') {
      await new Promise(resolve => {
        const timer = setTimeout(resolve, 25000);
        payload.signal.addEventListener('abort', () => { clearTimeout(timer); resolve(); }, { once: true });
      });
    }
    if (mode() === 'changed') {
      const decision = await next();
      if (decision.kind !== 'enter') return decision;
      return { ...decision, messages: decision.messages.map(message => ({ ...message,
        content: message.content.map(block => block.type === 'text' ? { ...block, text: block.text + ' fixture edit' } : block) })) };
    }
    return next();
  });
}
