// Official local web API. Launch tokens and key values stay in this process.
let input = '';
for await (const chunk of process.stdin) input += chunk;
const request = JSON.parse(input);
input = '';
async function accountClient(url) {
  const origin = new URL(url).origin;
  const auth = await fetch(url, {redirect: 'manual', signal: AbortSignal.timeout(3000)});
  if (auth.status !== 303) throw new Error('auth');
  const cookie = auth.headers.getSetCookie().map(v => v.split(';')[0]).join('; ');
  if (!cookie) throw new Error('auth');
  return async (method, args = {}) => {
    const response = await fetch(origin + '/api/' + method, {method: 'POST', redirect: 'error',
      headers: {Cookie: cookie, Origin: origin, 'Content-Type': 'application/json'},
      body: JSON.stringify({type: 'client-request', rpcId: 'hgs-account', method, payload: {args}}), signal: AbortSignal.timeout(5000)});
    if (!response.ok) throw new Error('transport');
    const r = (await response.json()).result;
    if (!r?.ok) throw new Error('account');
    return r.value;
  };
}
async function apiKeyReference(call) {
  const [routes, settings] = await Promise.all([call('llm/listConfigurableProviders'), call('settings/describe')]);
  const route = routes.find(v => v.provider === 'deepseek-official');
  const ns = settings.namespaces.find(v => v.ns === route?.settingsNs);
  const value = route?.settingsPath.reduce((v, key) => v?.[key], ns?.value);
  if (typeof value?.apiKeyEnv !== 'string' || !value.apiKeyEnv) throw new Error('route');
  return value.apiKeyEnv;
}

// The active key, read with the released parser from the file source only;
// environment overrides must never merge by a different credential sitting in
// the file underneath them. The key stays in this process.
async function fileKey(home, ref, source) {
  if (source !== 'file') return '';
  try {
    const fs = await import('node:fs/promises');
    const {join} = await import('node:path');
    const {createRequire} = await import('node:module');
    const {pathToFileURL} = await import('node:url');
    let executable;
    for (const dir of process.env.PATH.split(':')) {
      try { executable = await fs.realpath(join(dir, 'dsh')); break; } catch {}
    }
    if (!executable) return '';
    const require = createRequire(executable);
    const {parseCredentialsDocument} = await import(pathToFileURL(require.resolve('@deepseek-ai/dsh-credentials-local')).href);
    const path = join(home, '.credentials.yaml');
    const stat = await fs.lstat(path);
    if (!stat.isFile() || stat.size > 1024 * 1024 || (stat.mode & 0o077)) return '';
    const key = parseCredentialsDocument(await fs.readFile(path, 'utf8'), path).refs.get(ref);
    return typeof key === 'string' ? key : '';
  } catch { return ''; }
}
// Only an opaque identity of the key leaves the machine.
async function keyIdentity(key) {
  if (!key) return {};
  const {createHash} = await import('node:crypto');
  return {account_id: 'api-key:' + createHash('sha256').update('deepseek-api-key\0' + key).digest('hex')};
}
