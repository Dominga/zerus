// Official DeepSeek balance of an API-key account (GET /user/balance). The key
// goes only to the official API host: production callers never pass a URL,
// and a redirect is refused rather than followed. Only the public balance
// fields return; hgs validates and projects them.
const DEEPSEEK_BALANCE_URL = 'https://api.deepseek.com/user/balance';
const DEEPSEEK_BALANCE_BYTES = 65536;
async function apiBalance(key, url = DEEPSEEK_BALANCE_URL, timeout = 5000) {
  const signal = AbortSignal.timeout(timeout);
  const response = await fetch(url, {headers: {Authorization: 'Bearer ' + key, Accept: 'application/json'},
    redirect: 'error', signal});
  if (!response.ok || !response.body) {
    await response.body?.cancel();
    throw new Error('balance');
  }
  const reader = response.body.getReader();
  const chunks = [];
  let size = 0;
  for (;;) {
    const {done, value} = await reader.read();
    if (done) break;
    size += value.length;
    if (size > DEEPSEEK_BALANCE_BYTES) {
      await reader.cancel();
      throw new Error('balance');
    }
    chunks.push(value);
  }
  const reply = JSON.parse(Buffer.concat(chunks).toString('utf8'));
  if (!reply || typeof reply !== 'object' || !Array.isArray(reply.balance_infos)) throw new Error('balance');
  return {is_available: reply.is_available, balance_infos: reply.balance_infos.slice(0, 8)};
}
