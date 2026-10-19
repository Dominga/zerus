// Keeps one SocratiCode MCP server alive for a folder, so its
// index and file watcher run without an agent session. hgs starts it detached
// with HGS_SC_PATH (the folder) and HGS_SC_STATE (its state file); a file named
// like the state file plus ".stop" ends it. The server ends when its input
// closes, which also happens when this process dies.
import { spawn, spawnSync } from 'node:child_process';
import fs from 'node:fs';

const folder = process.env.HGS_SC_PATH;
const stateFile = process.env.HGS_SC_STATE;
const stopFile = stateFile + '.stop';
const state = { pid: process.pid, path: folder, phase: 'starting', detail: '', error: '', started_at: Date.now() / 1000 };
const save = (changes) => {
  Object.assign(state, changes, { updated_at: Date.now() / 1000 });
  state.detail = String(state.detail).slice(0, 2000);
  state.error = String(state.error).slice(0, 2000);
  try { fs.writeFileSync(stateFile + '.tmp', JSON.stringify(state)); fs.renameSync(stateFile + '.tmp', stateFile); } catch {}
};
try { fs.unlinkSync(stopFile); } catch {}
save({});

const windows = process.platform === 'win32';
// Windows runs npx.cmd through its shell: one fixed command line, no arguments.
const server = spawn(windows ? 'npx.cmd -y --prefer-online socraticode@latest' : 'npx', windows ? [] : ['-y', '--prefer-online', 'socraticode@latest'], {
  cwd: folder, shell: windows, windowsHide: true, stdio: ['pipe', 'pipe', 'ignore'],
  env: { ...process.env, SOCRATICODE_WATCHER: process.env.SOCRATICODE_WATCHER || 'auto' },
});
let ended = false;
const finish = (changes) => {
  if (ended) return;
  ended = true;
  save(changes);
  try { server.stdin.end(); } catch {}
  setTimeout(() => {
    if (server.exitCode === null && windows && server.pid) spawnSync('taskkill', ['/pid', String(server.pid), '/T', '/F'], { windowsHide: true });
    else if (server.exitCode === null) server.kill();
    process.exit(0);
  }, 8000).unref();
};
server.on('error', (error) => finish({ phase: 'failed', error: `SocratiCode did not start: ${error.message}` }));
server.on('exit', () => { if (!ended) finish({ phase: 'failed', error: state.error || 'SocratiCode stopped.' }); setTimeout(() => process.exit(0), 500); });

// MCP over stdio: one JSON message per line.
let next = 1, buffer = '';
const waiting = new Map();
server.stdout.setEncoding('utf8');
server.stdout.on('data', (chunk) => {
  buffer += chunk;
  if (buffer.length > 8 * 1024 * 1024) buffer = '';
  let at;
  while ((at = buffer.indexOf('\n')) >= 0) {
    const line = buffer.slice(0, at).trim();
    buffer = buffer.slice(at + 1);
    if (!line) continue;
    let message;
    try { message = JSON.parse(line); } catch { continue; }
    const reply = waiting.get(message.id);
    if (reply) { waiting.delete(message.id); reply(message); }
  }
});
const request = (method, params) => new Promise((resolve) => {
  const id = next++;
  waiting.set(id, resolve);
  server.stdin.write(JSON.stringify({ jsonrpc: '2.0', id, method, params }) + '\n');
});
const text = (message) => (message.result?.content || []).map((part) => part.text || '').join('\n') || message.error?.message || '';
const call = async (name, args) => text(await request('tools/call', { name, arguments: { projectPath: folder, ...args } }));

setInterval(() => { if (fs.existsSync(stopFile)) { try { fs.unlinkSync(stopFile); } catch {} finish({ phase: 'stopped' }); } }, 2000);

const main = async () => {
  const init = await request('initialize', { protocolVersion: '2025-06-18', capabilities: {}, clientInfo: { name: 'zerus', version: '1' } });
  if (init.error) return finish({ phase: 'failed', error: init.error.message });
  server.stdin.write(JSON.stringify({ jsonrpc: '2.0', method: 'notifications/initialized' }) + '\n');
  let status = await call('codebase_status', {});
  // An existing index catches up and is watched; a new one is built first,
  // and SocratiCode watches it once it completes.
  const indexed = /^Collection:/m.test(status) && !/in progress/i.test(status);
  save({ phase: indexed ? 'watching' : 'indexing', detail: status });
  const answer = await call(indexed ? 'codebase_watch' : 'codebase_index', indexed ? { action: 'start' } : {});
  if (!/Indexing started|in progress|Started watching|Already watched/i.test(answer)) return finish({ phase: 'failed', error: answer });
  save({ detail: answer });
  for (;;) {
    await new Promise((resolve) => setTimeout(resolve, 15000));
    if (ended) return;
    status = await call('codebase_status', {});
    save({ phase: /in progress/i.test(status) ? 'indexing' : 'watching', detail: status });
  }
};
main().catch((error) => finish({ phase: 'failed', error: error.message }));
