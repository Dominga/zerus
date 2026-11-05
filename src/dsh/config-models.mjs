// Read the composed DeepSeek profile YAML and print only its API-key model
// catalog (task #19). The caller runs `dsh --profile hgs --dump-config`,
// which composes the profile without starting a host or creating a session,
// and hands the bounded text to this process. The `deepseek-official` route
// (`@deepseek-ai/dsh-llm-deepseek-api-key`, row `llm-deepseek`) serves the
// `models` its config lists; without them, the installed transport's own
// bundled catalog. Both resolve through the transport's
// `resolveAdapterOptions`, so this reports exactly what dsh would offer.
// `!!js` values stay opaque strings and are never evaluated; unrelated
// config values never leave this process.
import { createRequire } from 'node:module';
import fs from 'node:fs/promises';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

let input = '';
for await (const chunk of process.stdin) input += chunk;
if (input.length > 512 * 1024) fail('input');

const request = JSON.parse(input);
input = '';
if (typeof request.dump !== 'string' || !request.dump) fail('input');

async function packageRoot() {
  if (typeof request.packageRoot === 'string' && request.packageRoot) {
    return request.packageRoot;
  }
  // The helper inherits the launcher's PATH, so this resolves the same
  // executable `Command::new("dsh")` would start.
  for (const directory of (process.env.PATH ?? '').split(path.delimiter)) {
    for (const name of ['dsh', 'dsh.cmd']) {
      try {
        const executable = await fs.realpath(path.join(directory, name));
        if (!(await fs.stat(executable)).isFile()) continue;
        const found = await walk(executable) ?? await walk(path.join(directory, 'node_modules', '@deepseek-ai', 'dsh', 'package.json'));
        if (found) return found;
      } catch {}
    }
  }
  return null;
}

async function walk(file) {
  let directory = path.dirname(file);
  for (let depth = 0; depth < 6; depth += 1) {
    try {
      const manifest = JSON.parse(await fs.readFile(path.join(directory, 'package.json'), 'utf8'));
      if (manifest?.name === '@deepseek-ai/dsh') return directory;
    } catch {}
    const parent = path.dirname(directory);
    if (parent === directory) break;
    directory = parent;
  }
  return null;
}

const root = await packageRoot();
if (!root) fail('package');
const require = createRequire(path.join(root, 'package.json'));
let yaml;
try {
  yaml = require('js-yaml');
} catch {
  fail('yaml');
}

const jsExpression = new yaml.Type('tag:yaml.org,2002:js', {
  kind: 'scalar',
  resolve: data => typeof data === 'string',
  construct: data => ({ expression: String(data) }),
});
const schema = yaml.JSON_SCHEMA.extend([jsExpression]);
let document;
try {
  document = yaml.load(request.dump, { schema });
} catch {
  fail('yaml');
}
if (!Array.isArray(document)) fail('shape');

// A value the native loader computes later (`!!js`) is unknown here.
const computed = value => value !== null && typeof value === 'object' && typeof value.expression === 'string';

// Later layers may repeat the provider row; their configs merge in order.
let config = null;
for (const row of document) {
  if (!row || row.name !== '@deepseek-ai/dsh-llm-deepseek-api-key') continue;
  if (row.disabled === true) {
    config = null;
    continue;
  }
  if (computed(row.disabled)) fail('computed');
  config = { ...(config ?? {}), ...(row.config && typeof row.config === 'object' && !Array.isArray(row.config) ? row.config : {}) };
}
if (config === null) fail('provider');

// Only the fields that decide the catalog go to the resolver.
const fields = {};
for (const key of ['models', 'thinking', 'reasoningEffort']) {
  if (config[key] === undefined) continue;
  if (computed(config[key])) fail('computed');
  fields[key] = config[key];
}
if (Array.isArray(fields.models) && fields.models.some(entry => !entry || typeof entry !== 'object' || computed(entry.id))) fail('computed');

let transport;
try {
  transport = await import(pathToFileURL(require.resolve('@deepseek-ai/dsh-llm-deepseek')).href);
} catch {
  fail('transport');
}
let resolved;
try {
  resolved = transport.resolveAdapterOptions(fields, undefined);
} catch {
  fail('config');
}
const models = Array.isArray(resolved?.models) ? resolved.models : [];
// The transport's exact-model metadata (its `modelInfo`): with thinking
// disabled only `off`; otherwise `off`, `low`, `high` and `max`, the default
// following `reasoningEffort` and `high` without one.
const efforts = fields.thinking === 'disabled' ? ['off'] : ['off', 'low', 'high', 'max'];
const preferred = fields.thinking === 'disabled' ? 'off' : (['off', 'low', 'max'].includes(fields.reasoningEffort) ? fields.reasoningEffort : 'high');
const out = [];
for (const model of models.slice(0, 64)) {
  if (!model || typeof model.id !== 'string' || !model.id || model.id.length > 120 || /[\u0000-\u001f\u007f]/.test(model.id)) continue;
  const name = typeof model.name === 'string' && model.name.trim() ? model.name.trim().slice(0, 120) : model.id;
  out.push({ provider: 'deepseek-official', id: model.id, name, efforts, defaultEffort: preferred });
}
if (out.length === 0) fail('empty');
process.stdout.write(JSON.stringify({ ok: true, models: out, truncated: models.length > 64 }));

function fail(reason) {
  process.stdout.write(JSON.stringify({ ok: false, reason }));
  process.exit(1);
}
