// Add permission support to a resident legacy bridge using the released
// authenticated web API. Keep its owned agents, questions, socket and host
// generation intact; never reload it. account-client.mjs supplies request and
// the normal launch-token exchange. Only sanitized option metadata leaves this
// process: never launch tokens, cookies, native RPC payloads or credentials.
try {
  const { url, sessionId, operation, pending, preset, explicit } = request;
  const catalogOperation = operation === 'catalog';
  if (!catalogOperation && !/^session-[0-9a-f-]{36}$/.test(sessionId)) throw new Error('identity');
  const call = await accountClient(url);
  const catalog = await call('permissionPresets/catalog');
  // Auto is a current-session-only integration and never an account/launch
  // default. defaultOptions omits it; filter anyway for older harnesses.
  const options = (catalog.defaultOptions ?? catalog.options ?? [])
    .filter(option => option?.value !== 'auto');
  if (options.length > 64) throw new Error('oversized');
  const listed = options.map(option => {
    const value = option?.value;
    // Native keys may contain internal spaces, Unicode or punctuation; the
    // native /permission handler trims the whole remainder, so reject only
    // values that trim differently, carry controls or exceed the common bound.
    if (typeof value !== 'string' || !value || Buffer.byteLength(value, 'utf8') > 128
        || value.trim() !== value || /[\u0000-\u001f\u007f]/.test(value)) {
      throw new Error('preset');
    }
    const label = typeof option.name === 'string' && option.name ? option.name : value;
    const description = typeof option.description === 'string' && option.description
      ? option.description : undefined;
    if (label.length > 160 || (description?.length ?? 0) > 480) throw new Error('oversized');
    return { value, label, ...(description ? { description } : {}) };
  });
  const selected = typeof preset === 'string' ? preset : '';
  // Legacy check/apply carry no preset: their target is the published Bypass
  // preset itself. Only the preset operations require an explicit, offered key.
  const presetOperation = operation === 'check-preset' || operation === 'apply-preset';
  if (presetOperation) {
    if (!selected || Buffer.byteLength(selected, 'utf8') > 128
        || selected.trim() !== selected || /[\u0000-\u001f\u007f]/.test(selected)) {
      throw new Error('preset');
    }
    if (!listed.some(option => option.value === selected)) throw new Error('preset');
  }
  const emit = value => {
    const answer = JSON.stringify(value);
    if (answer.length > 48 * 1024) throw new Error('oversized');
    process.stdout.write(answer);
  };
  if (operation === 'catalog') {
    const fallbackDefault = typeof catalog.defaultPreset === 'string'
      && listed.some(option => option.value === catalog.defaultPreset) ? catalog.defaultPreset : null;
    emit({ ok: true, options: listed, default: fallbackDefault });
  } else if (operation === 'check' || operation === 'check-preset') {
    const list = await call('session/list', { _request: {} });
    const session = list.items.find(item => item.sessionId === sessionId);
    emit({ ok: true, needs_setup: !session?.agentAvailable || pending === true });
  } else if (operation === 'apply' || operation === 'apply-preset') {
    const target = operation === 'apply' ? 'danger-full-access' : selected;
    if (!listed.some(option => option.value === target)) throw new Error('preset');
    // Read the durable result first: an earlier uncertain acknowledgement must
    // not repeat a permission command that already succeeded. A session that
    // already holds the target needs no liveness lookup at all.
    const current = await call('session/projections', { request: { sessionId } });
    let skipped = false;
    if (current?.values.permissions?.currentValue !== target) {
      // session/list has no exact-session lookup; read it only to prove the
      // agent is live and idle before changing a preset.
      const list = await call('session/list', { _request: {} });
      const session = list.items.find(item => item.sessionId === sessionId);
      if (!session?.agentAvailable) throw new Error('state');
      if (session.running) {
        // An ordinary account default or ensure must never switch a live
        // agent. Only an explicit launch/resume or a pending recovery may.
        if (pending === true || explicit === true) throw new Error('busy');
        skipped = true;
      } else {
        const commands = await call('commands/list', { agentId: sessionId });
        if (!commands.some(command => command.name === 'permission'
            && command.definitionId === '@deepseek-ai/dsh-permission-presets')) throw new Error('command');
        const result = await call('commands/execute', {
          agentId: sessionId, line: '/permission ' + target, submittedAttachments: [],
        });
        if (result?.result.kind !== 'success') throw new Error('result');
      }
    }
    if (skipped) {
      emit({ ok: true, skipped: true });
    } else {
      const confirmed = await call('session/projections', { request: { sessionId } });
      if (confirmed?.values.permissions?.currentValue !== target) throw new Error('confirmation');
      emit({ ok: true });
    }
  } else throw new Error('operation');
} catch (error) {
  const reasons = ['identity', 'preset', 'state', 'busy', 'command', 'result', 'confirmation',
    'operation', 'oversized'];
  const message = String(error?.message ?? '');
  const reason = reasons.includes(message) ? message
    : ['auth', 'transport', 'account'].includes(message) ? message : 'unknown';
  process.stdout.write(JSON.stringify({ ok: false, reason }));
  process.exitCode = 1;
}
