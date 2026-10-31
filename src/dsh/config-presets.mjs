// Read the composed DeepSeek profile YAML and print only its permission
// preset table. The caller runs `dsh --profile hgs --dump-config`, which
// composes the profile without starting a host or creating a session, and
// hands the bounded text to this process. The native parser dialect (JSON
// schema plus the package's `!!js` scalar tag) is used without evaluating any
// expression; unrelated config values never leave this process.
import { createRequire } from 'node:module';
import fs from 'node:fs/promises';
import path from 'node:path';

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
    try {
      const executable = await fs.realpath(path.join(directory, 'dsh'));
      if (!(await fs.stat(executable)).isFile()) continue;
      const found = await walk(executable);
      if (found) return found;
    } catch {}
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
let yaml;
try {
  yaml = createRequire(path.join(root, 'package.json'))('js-yaml');
} catch {
  fail('yaml');
}

// `!!js` values are expressions evaluated by the native loader later. Here
// they stay opaque strings: the permission table itself never uses the tag.
const jsExpression = new yaml.Type('tag:yaml.org,2002:js', {
  kind: 'scalar',
  resolve: data => typeof data === 'string',
  construct: data => String(data),
});
const schema = yaml.JSON_SCHEMA.extend([jsExpression]);
let document;
try {
  document = yaml.load(request.dump, { schema });
} catch {
  fail('yaml');
}
if (!Array.isArray(document)) fail('shape');

// Later layers may repeat the plugin row; merge their preset tables in order.
const presets = new Map();
let defaultPreset = null;
for (const row of document) {
  if (!row || row.name !== '@deepseek-ai/dsh-permission-presets') continue;
  const config = row.config;
  if (!config || typeof config !== 'object' || Array.isArray(config)) continue;
  const table = config.presets;
  if (table !== undefined && (typeof table !== 'object' || Array.isArray(table))) fail('presets');
  for (const [value, spec] of Object.entries(table ?? {})) {
    // Native preset keys are configured strings, not tokens: internal spaces,
    // Unicode and punctuation are valid. The native /permission handler trims
    // the whole remainder, so edges that are not already trimmed are
    // unselectable and the catalog would claim a choice nobody can apply.
    if (typeof value !== 'string' || !value || Buffer.byteLength(value, 'utf8') > 128
        || value.trim() !== value || /[\u0000-\u001f\u007f]/.test(value)) fail('preset-name');
    if (!spec || typeof spec !== 'object' || Array.isArray(spec)) fail('preset');
    const label = typeof spec.name === 'string' && spec.name.trim() ? spec.name.trim() : value;
    const description = typeof spec.description === 'string' && spec.description.trim()
      ? spec.description.trim() : undefined;
    if (label.length > 160 || (description?.length ?? 0) > 480) fail('preset-text');
    presets.set(value, { value, label, ...(description ? { description } : {}) });
  }
  if (typeof config.defaultPreset === 'string' && config.defaultPreset) {
    defaultPreset = config.defaultPreset;
  }
}
if (presets.size === 0) fail('empty');
if (presets.size > 64) fail('too-many');
if (defaultPreset !== null && !presets.has(defaultPreset)) defaultPreset = null;
process.stdout.write(JSON.stringify({ ok: true, presets: [...presets.values()], default: defaultPreset }));

function fail(reason) {
  process.stdout.write(JSON.stringify({ ok: false, reason }));
  process.exit(1);
}
