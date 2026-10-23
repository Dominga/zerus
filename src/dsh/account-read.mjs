// Read native public account projections through the released authenticated web API.
// Launch credentials arrive on stdin and never leave this process; an API key
// goes only to DeepSeek's own balance endpoint. Without a reachable native UI
// only the API key file and environment are read.
const result = {identity: {}, windows: [], source: 'DeepSeek Harness', status: 'unavailable'};
const DEFAULT_REF = 'DEEPSEEK_API_KEY';
try {
  let ref = DEFAULT_REF, source = 'file', call = null;
  // host.log outlives its host: an unreachable UI reads the key as if there were none.
  if (request.url) call = await accountClient(request.url).catch(() => null);
  if (call) {
    try {
      ref = await apiKeyReference(call);
      const credential = (await call('credentials/describe', {refs: [ref]}))[ref];
      if (credential?.configured) { result.api_key_configured = true; source = credential.source; }
    } catch {}
    if (!result.api_key_configured) {
      const state = await call('account/getState');
      if (state.status === 'signed-out') result.status = 'signed_out';
      else {
        const client = {version: '0.2.1-rc.2', locale: 'en', timezoneOffsetSeconds: -new Date().getTimezoneOffset() * 60};
        const [profile, balance] = await Promise.allSettled([call('account/getProfile', {client}), call('account/getBalance', {client})]);
        const clean = v => typeof v === 'string' ? v.replace(/[\x00-\x1f\x7f]/g, '').slice(0,256) : '';
        if (profile.value?.status === 'ready') result.identity = {name: clean(profile.value.value.name), email: clean(profile.value.value.contact), auth_method: 'DeepSeek account'};
        if (balance.value?.status === 'ready') {
          result.balances = [...balance.value.value.map(v => ({...v,kind:'Wallet'})), ...(balance.value.bonusWallets ?? []).map(v => ({...v,kind:'Bonus'}))]
            .filter(v => ['USD','CNY'].includes(v.currency) && /^-?\d+(\.\d+)?$/.test(v.balance))
            .map(v => ({currency:v.currency,balance:v.balance,kind:v.kind}));
          result.status = 'ok';
        }
      }
    }
  }
  if (!call || result.api_key_configured) {
    // The environment wins over the file, as in the harness.
    const key = source === 'env' || process.env[ref] ? process.env[ref] : await apiKey(request.home, ref);
    if (typeof key === 'string' && key) {
      result.api_key_configured = true;
      result.status = 'configured';
      result.identity = {auth_method: 'API key', account_id: await keyId(key)};
      const balances = await apiBalance(key);
      if (balances) { result.balances = balances; result.status = 'ok'; }
    }
  }
} catch {}
// No process.exit: on Windows, Node 24 can abort with a libuv assertion when
// it exits right after fetch, and the nonzero status discards this answer.
// The script ends on its own once stdout is flushed.
process.stdout.write(JSON.stringify(result));

// The released harness's own credentials parser, found next to its executable:
// a symlink into the package on Unix, an npm shim beside node_modules on Windows.
async function credentialsParser() {
  const fs = await import('node:fs/promises');
  const {join, dirname, delimiter} = await import('node:path');
  const {createRequire} = await import('node:module');
  const {pathToFileURL} = await import('node:url');
  let entry;
  for (const dir of (process.env.PATH ?? '').split(delimiter)) {
    if (!dir) continue;
    try {
      if (process.platform === 'win32') {
        await fs.access(join(dir, 'dsh.cmd'));
        entry = join(dir, 'node_modules', '@deepseek-ai', 'dsh', 'lib', 'bin.js');
        await fs.access(entry);
      } else entry = await fs.realpath(join(dir, 'dsh'));
      break;
    } catch { entry = undefined; }
  }
  if (!entry) return null;
  const require = createRequire(entry);
  return (await import(pathToFileURL(require.resolve('@deepseek-ai/dsh-credentials-local')).href)).parseCredentialsDocument;
}

async function apiKey(home, ref) {
  try {
    const fs = await import('node:fs/promises');
    const {join} = await import('node:path');
    const parse = await credentialsParser();
    if (!parse) return '';
    const path = join(home, '.credentials.yaml');
    const stat = await fs.lstat(path);
    // Windows has no Unix permission bits; its profile folder is private.
    if (!stat.isFile() || stat.size > 1024 * 1024 || (process.platform !== 'win32' && (stat.mode & 0o077))) return '';
    const key = parse(await fs.readFile(path, 'utf8'), path).refs.get(ref);
    return typeof key === 'string' ? key : '';
  } catch { return ''; }
}

// Only an opaque identity leaves the machine.
async function keyId(key) {
  const {createHash} = await import('node:crypto');
  return 'api-key:' + createHash('sha256').update('deepseek-api-key\0' + key).digest('hex');
}

// GET https://api.deepseek.com/user/balance: {is_available, balance_infos:
// [{currency, total_balance, granted_balance, topped_up_balance}]}.
async function apiBalance(key) {
  try {
    const response = await fetch('https://api.deepseek.com/user/balance', {
      headers: {Authorization: 'Bearer ' + key, Accept: 'application/json'}, redirect: 'error', signal: AbortSignal.timeout(6000)});
    if (!response.ok) return null;
    const data = await response.json();
    const balances = (Array.isArray(data?.balance_infos) ? data.balance_infos : [])
      .filter(v => ['USD','CNY'].includes(v?.currency) && /^-?\d+(\.\d+)?$/.test(String(v.total_balance)))
      .map(v => ({currency: v.currency, balance: String(v.total_balance), kind: 'API'}));
    return balances.length ? balances : null;
  } catch { return null; }
}
