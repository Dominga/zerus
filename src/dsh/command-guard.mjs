// Phase-one lexical guard. Keep this algorithm and state/command_guard.rs in
// step through tests/fixtures/command-guard.json. No evaluation or filesystem I/O.
const limit = 256 * 1024;
const readonly = new Set('home pid pshome host executioncontext psversiontable psculture psuiculture psedition shellid true false error consolefilename'.split(' '));
const destructive = new Set(['rm', 'rmdir', 'rd', 'del', 'erase', 'remove-item', 'ri', 'move-item', 'mi', 'mv', 'move', 'robocopy', '[system.io.directory]::delete', '[io.directory]::delete']);
const shells = new Set(['sh', 'bash', 'zsh', 'dash', 'pwsh', 'powershell', 'cmd']);
const boundaries = new Set([';', '\n', '|', '&', '(', ')', '{', '}']);
const reject = (rule, detail) => ({rule, message: `Zerus blocked this command: ${detail}`});
const tooComplex = () => reject('protected-path', 'the command exceeds the text inspection limit. Split it into smaller, explicit commands.');
const base = text => text.toLowerCase().replaceAll('\\', '/').split('/').at(-1).replace(/\.exe$/, '');
const variable = text => /^\$(?:\{)?(?:(?:global|script|local|private):)?([a-z_][a-z0-9_]*)(?:\})?$/i.exec(text)?.[1]?.toLowerCase();
const assignedValue = (assigned, name) => assigned.get('ps:' + name.toLowerCase()) ?? assigned.get('sh:' + name);

// Quotes remain data, except arguments explicitly passed to a shell's -c.
// Backslashes in Windows paths are not POSIX escape characters here.
function lex(source) {
  const tokens = [];
  let word = '', quoted = false, active = false;
  const flush = () => { if (active) tokens.push({text: word, quoted}); word = ''; quoted = false; active = false; };
  for (let i = 0; i < source.length; i++) {
    const c = source[i];
    if (c === '#' && !active) { while (i < source.length && source[i] !== '\n') i++; flush(); tokens.push({text: '\n'}); }
    else if (c === '"' || c === "'") {
      active = quoted = true;
      for (i++; i < source.length; i++) {
        if (source[i] === c) {
          if (c === "'" && source[i + 1] === c) { word += c; i++; continue; }
          break;
        }
        if (c === '"' && (source[i] === '`' || source[i] === '\\' && !/^(?:[a-z]:|[~$%]|\\\\)/i.test(word)) && source[i + 1] === '"') i++;
        word += source[i];
      }
    } else if (c === '$' && source[i + 1] === '{') {
      active = true; word += '${'; i += 2;
      while (i < source.length && source[i] !== '}') word += source[i++];
      if (i < source.length) word += '}';
    } else if (c === '`' && i + 1 < source.length) {
      if (source[i + 1] === '\r') i++;
      if (source[i + 1] === '\n') i++;
      else { active = true; word += source[++i]; }
    } else if (c === '\\' && source[i + 1] === '\n') i++;
    else if (';|&(){}=,\n'.includes(c)) { flush(); tokens.push({text: c}); }
    else if (/\s/.test(c)) flush();
    else { active = true; word += c; }
  }
  flush(); return tokens;
}

// Pure lexical normalization, including dot segments; never resolve symlinks.
function normalize(value) {
  const path = value.replaceAll('\\', '/').toLowerCase().replace(/^\/\/[?.]\//, '').replace(/\/+$/, '');
  const windows = /^[a-z]:/.test(path);
  const parts = [];
  for (let part of path.split('/')) {
    if (part === '.') continue;
    if (part === '..' && parts.length && parts.at(-1) !== '' && !parts.at(-1).endsWith(':')) parts.pop();
    else if (part !== '..') {
      if (windows) part = part.replace(/[. ]+$/, '');
      if (part || !parts.length || path.startsWith('//') && parts.length < 2) parts.push(part);
    }
  }
  return parts.join('/');
}

function protectedPath(value, home) {
  let p = value.replaceAll('\\', '/').toLowerCase();
  const homeRef = /^(?:~|\$(?:\{(?:(?:env|global|script|local):)?(?:home|userprofile)\}|(?:(?:env|global|script|local):)?(?:home|userprofile))|%(?:userprofile|home)%|%homedrive%%homepath%|\$env:homedrive\$env:homepath)($|\/)/;
  p = p.replace(homeRef, (_, slash) => home + slash);
  p = p.replace(/^(?:\$\{?env:(?:windir|systemroot)\}?|%(?:windir|systemroot)%)($|\/)/, (_, slash) => 'c:/windows' + slash);
  p = normalize(p);
  while (p.endsWith('/*')) p = p.slice(0, -2);
  if (!p || /^[a-z]:$/.test(p)) return true;
  if (/^\/\/[^/]+\/[^/]+$/.test(p)) return true;
  const system = ['/bin', '/boot', '/dev', '/etc', '/home', '/lib', '/lib64', '/opt', '/proc', '/root', '/run', '/sbin', '/sys', '/usr', '/var', '/users', '/system', '/library', '/applications'];
  if (system.includes(p) || /^\/(?:home|users)\/[^/]+$/.test(p)) return true;
  if (/^\/(?:etc|usr|bin|sbin|boot|lib|lib64|system)(?:\/|$)/.test(p)) return true;
  if (/^[a-z]:\/(?:windows|program files(?: \(x86\))?|programdata)(?:\/|$)/.test(p)) return true;
  if (/^[a-z]:\/users(?:\/[^/]+)?$/.test(p)) return true;
  if (p === home) return true;
  if (p.startsWith(home + '/')) {
    const tail = p.slice(home.length + 1);
    if (!tail.includes('/') && /[*?]/.test(tail)) return true;
    // Whole profile directories and credentials, including their contents.
    if (/^(?:\.ssh|\.gnupg|\.aws|\.azure|\.config|\.local|\.claude|\.codex|\.dsh|appdata)(?:\/|$)/.test(tail)) return true;
    if (['documents', 'desktop', 'downloads', 'pictures', 'music', 'videos', 'library', '.cache'].includes(tail)) return true;
  }
  return false;
}

function pathRule(value, home, assigned) {
  if (protectedPath(value, home)) return 'protected-path';
  const ref = /^(?:\$\{([a-z_][a-z0-9_]*)\}|\$([a-z_][a-z0-9_]*)|%([a-z_][a-z0-9_]*)%)($|[\\/])/i.exec(value);
  if (ref && assignedValue(assigned, ref[1] ?? ref[2] ?? ref[3]) === false) return 'protected-path';
  // Only a statically nonempty literal earlier in this straight-line scope
  // counts. A conditional/loop, reassignment, or statement boundary with a
  // dynamic value cannot establish safety. ${VAR:?} is an explicit shell guard.
  const match = /^(\$(?:\{([a-z_][a-z0-9_]*)(:\?)?\}|([a-z_][a-z0-9_]*))|%([a-z_][a-z0-9_]*)%)[\\/]\**[\\/]*$/i.exec(value);
  if (match && !match[3] && assignedValue(assigned, match[2] ?? match[4] ?? match[5]) === undefined) return 'empty-variable';
  return null;
}

function inspect(command, home, depth) {
  if (depth > 8) return tooComplex();
  const tokens = lex(command), assigned = new Map();
  let head = true, paramDepth = 0, nesting = 0, flow = false;
  for (let i = 0; i < tokens.length; i++) {
    const t = tokens[i], s = t.text.toLowerCase(), next = tokens[i + 1]?.text;
    const v = !t.quoted && variable(s.replace(/^(?:\[[^\]]+\])+/, '').replace(/^(?:\+\+|--)/, '').replace(/[+*/%-]$/, ''));
    const compound = ['+', '-', '*', '/', '%'].includes(next) && tokens[i + 2]?.text === '=';
    const write = next === '=' && tokens[i + 2]?.text !== '=' || compound || ['++', '--'].some(op => s.endsWith(op) || s.startsWith(op));
    const name = v || (!t.quoted && variable(s.replace(/(?:\+\+|--)$/, '')));
    if (name && readonly.has(name) && (write || next?.toLowerCase() === 'in' || paramDepth))
      return reject('readonly-variable', `writing PowerShell automatic variable $${name} is unsafe. Use another variable name, such as $appHome.`);
    if (!t.quoted && (next === '=' || compound)) {
      const key = v ? 'ps:' + v : (/^[a-z_][a-z0-9_]*$/i.test(s) ? 'sh:' + t.text : null);
      if (key) {
        assigned.delete(key);
        const val = tokens[i + 2], end = tokens[i + 3];
        if (head && !compound && !/[+*/%-]$/.test(s) && !flow && !nesting && val?.text && !/[$%`]/.test(val.text) && (!end || boundaries.has(end.text))) assigned.set(key, !protectedPath(val.text, home));
      }
    }
    if (!t.quoted && boundaries.has(s)) {
      head = true;
      if (['(', '{'].includes(s)) { nesting++; assigned.clear(); }
      if ([')', '}'].includes(s)) { nesting = Math.max(0, nesting - 1); assigned.clear(); if (paramDepth > nesting) paramDepth = 0; }
      continue;
    }
    if (head && s === 'param' && next === '(') paramDepth = nesting + 1;
    if (!head) continue;
    const cmd = base(s);
    if (['if', 'elif', 'for', 'foreach', 'while', 'until', 'case', 'function', 'try', 'catch'].includes(cmd)) { flow = true; assigned.clear(); }
    if (['set-variable', 'sv', 'new-variable', 'nv', 'clear-variable', 'clv', 'remove-variable', 'rv', 'unset', 'read'].includes(cmd)) assigned.clear();
    if (['sudo', 'command', 'exec', 'env', 'then', 'do', 'else'].includes(cmd) || s.startsWith('-')) continue;
    head = false;
    let end = i + 1;
    if (cmd.endsWith('::delete') && tokens[end]?.text === '(') end++;
    const start = end;
    while (end < tokens.length && (tokens[end].quoted || !boundaries.has(tokens[end].text))) end++;
    const args = tokens.slice(start, end);
    if (shells.has(cmd)) {
      const at = args.findIndex(a => /^(?:-c|-command|\/c|-lc|-ic)$/i.test(a.text));
      if (at >= 0) {
        const result = inspect(args.slice(at + 1).map(a => a.text).join(' '), home, depth + 1);
        if (result) return result;
      }
    }
    if (['set-variable', 'sv', 'new-variable', 'nv'].includes(cmd)) {
      const at = args.findIndex(a => /^-n(?:ame)?$/i.test(a.text));
      const name = (at >= 0 ? args[at + 1]?.text : args[0]?.text)?.toLowerCase().replace(/^(?:global|script|local|private):/, '');
      if (readonly.has(name)) return reject('readonly-variable', `writing PowerShell automatic variable $${name} is unsafe. Use another variable name, such as $appHome.`);
    }
    if (!destructive.has(cmd)) continue;
    const paths = [];
    for (let a = 0; a < args.length; a++) {
      const value = args[a].text.replace(/^-(?:literalpath|path):/i, '');
      if (/^-(?:destination|filter|include|exclude|erroraction|warningaction|ev|ea)$/i.test(value)) { a++; continue; }
      if (value === ',' || value === '=' || value.startsWith('-') || /^\/(?:s|q|f|mir|purge|e)$/i.test(value)) continue;
      paths.push(value);
    }
    const targets = cmd === 'robocopy' ? (args.some(a => /^\/(?:mir|purge)$/i.test(a.text)) ? paths.slice(1, 2) : [])
      : ['mv', 'move', 'move-item', 'mi'].includes(cmd) && paths.length > 1 && !args.some(a => /^-destination$/i.test(a.text)) ? paths.slice(0, -1) : paths;
    for (const target of targets) {
      const rule = pathRule(target, home, assigned);
      if (rule) return reject(rule, rule === 'empty-variable'
        ? 'a deletion path can become a filesystem root when its variable is empty. Assign a nonempty literal path first or use an explicit ${VAR:?} check.'
        : 'deleting, moving, or mirroring a protected home, root, system, or profile directory is unsafe. Target an explicit disposable subdirectory instead.');
    }
  }
  return null;
}

export function checkCommand(command, home) {
  if (typeof command !== 'string') return null;
  if (Buffer.byteLength(command, 'utf8') > limit) return tooComplex();
  return inspect(command, normalize(home), 0);
}
