// Executes the analyzer and middleware only, never the command strings.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import { checkCommand } from '../../src/dsh/command-guard.mjs';
import { apply } from '../../src/dsh/bridge.mjs';

const corpus = JSON.parse(fs.readFileSync(new URL('./command-guard.json', import.meta.url)));
for (const [rule, command] of corpus.blocked) {
  const result = checkCommand(command, corpus.home);
  assert.equal(result?.rule, rule, command);
  assert.ok(result.message.startsWith('Zerus blocked this command:'));
}
for (const command of corpus.allowed) assert.equal(checkCommand(command, corpus.home), null, command);
assert.equal(checkCommand('x'.repeat(256 * 1024 + 1), corpus.home).rule, 'protected-path');
assert.equal(checkCommand('echo ' + 'x'.repeat(17000) + '; rm -rf ~', corpus.home).rule, 'protected-path');
for (const home of ['/home/fixture', '/Users/fixture', 'D:\\profiles\\fixture']) {
  assert.equal(checkCommand(`Remove-Item '${home}' -Recurse`, home).rule, 'protected-path');
  assert.equal(checkCommand('rm -rf ~/.cache/pip', home), null);
}
assert.equal(checkCommand('sh -c '.repeat(10) + 'echo safe', corpus.home).rule, 'protected-path');

// Register the real listener without starting a host, sockets, agents or jobs.
const listeners = new Map();
apply({sessionController: {agents: {composeAgent() {}, serializeImageAdmission() {},
  selectForNextRequest() {}, selections: new WeakMap()}},
  get() {}, on(name, fn) { listeners.set(name, fn); }, effect() {}}, {});
const execute = listeners.get('tools/execute');
for (const name of ['bash', 'pwsh']) {
  for (const agent of [undefined, {session: {id: 'fixture-child'}, options: {permissionMode: 'bypass'}}]) {
    let calls = 0;
    const result = await execute({name, agent, arguments: {command: 'rm -rf ~'}}, () => { calls++; });
    assert.equal(calls, 0);
    assert.equal(result.isError, true);
    assert.equal(result.error.info.code, 'HGS_COMMAND_GUARD');
    assert.equal(result.content[0].text, 'Error: ' + result.error.message);
  }
  const expected = {isError: false, value: {exitCode: 0}};
  let calls = 0;
  const result = await execute({name, agent: {session: {id: 'fixture'}}, callId: 'safe',
    arguments: {command: 'Remove-Item out/tmp -Recurse'}}, () => { calls++; return expected; });
  assert.equal(calls, 1);
  assert.equal(result, expected);
}
const expected = {};
assert.equal(await execute({name: 'read', arguments: {command: 'rm -rf ~'}}, () => expected), expected);
console.log(`Command guard: ${corpus.blocked.length} blocked, ${corpus.allowed.length} allowed; bridge middleware passed.`);
