// Per-request usage from native DeepSeek session logs (session.vN.jsonl.zstd),
// read incrementally and only ever read. A log is a concatenation of complete,
// checksummed Zstandard frames, each holding whole JSONL lines; the reader
// resumes at a committed frame boundary, never splits a frame, stops at a torn
// final frame and keeps to the byte and time budget it is given. Only counts
// leave this process: no message text, paths or other record content.
import {openSync, readSync, fstatSync, closeSync} from 'node:fs';
import {zstdDecompressSync} from 'node:zlib';

const ZSTD_MAGIC = 0xFD2FB528;
const WINDOW = 64 * 1024 * 1024;

// The end of each complete frame from `start`, stopping before a torn one.
// A structurally invalid frame throws.
function frames(buffer) {
  const ends = [];
  let offset = 0;
  while (offset < buffer.length) {
    if (buffer.length - offset < 5) break;
    if (buffer.readUInt32LE(offset) !== ZSTD_MAGIC) throw new Error('format');
    let at = offset + 4;
    const descriptor = buffer[at++];
    if (descriptor & 24) throw new Error('format');
    const contentSizeFlag = descriptor >>> 6;
    const singleSegment = (descriptor & 32) !== 0;
    const dictionaryFlag = descriptor & 3;
    at += (singleSegment ? 0 : 1) + (dictionaryFlag === 3 ? 4 : dictionaryFlag)
      + (contentSizeFlag === 0 ? (singleSegment ? 1 : 0) : 1 << contentSizeFlag);
    let complete = false;
    while (at + 3 <= buffer.length) {
      const header = buffer.readUIntLE(at, 3);
      at += 3;
      const type = (header >>> 1) & 3;
      if (type === 3) throw new Error('format');
      at += type === 1 ? 1 : header >>> 3;
      if (at > buffer.length) break;
      if (header & 1) { complete = true; break; }
    }
    if (!complete) break;
    if (descriptor & 4) at += 4;
    if (at > buffer.length) break;
    ends.push(at);
    offset = at;
  }
  return ends;
}

const count = v => Number.isSafeInteger(v) && v > 0 ? v : 0;
function localDay(ms) {
  const d = new Date(ms);
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
}

// One log from `offset` within `budget` bytes. `cut` means a fork's inherited
// prefix ended here: everything counted for this log before it belongs to
// the parent session and must be dropped.
function readLog(file, sinceDay, budget, deadline) {
  const result = {key: file.key, offset: file.offset, done: false, cut: false, days: {}, read: 0};
  let fd;
  try {
    fd = openSync(file.path, 'r');
    const size = fstatSync(fd).size;
    if (size < file.offset) return {...result, error: 'rewritten'};
    const length = Math.min(size - file.offset, WINDOW, Math.max(budget, 0));
    const buffer = Buffer.alloc(length);
    let filled = 0;
    while (filled < length) {
      const n = readSync(fd, buffer, filled, length - filled, file.offset + filled);
      if (n === 0) break;
      filled += n;
    }
    const view = buffer.subarray(0, filled);
    // Past a committed frame, a foreign byte means the log was rewritten; at its start, it is not a log.
    if (view.length >= 4 && view.readUInt32LE(0) !== ZSTD_MAGIC) return {...result, error: file.offset ? 'rewritten' : 'format'};
    const ends = frames(view);
    result.read = filled;
    let start = 0;
    let text = '';
    let pending = {};
    let pendingCut = false;
    const add = (days, day, model, usage) => {
      const models = days[day] ??= {};
      const row = models[model] ??= [0, 0, 0, 0];
      row[0] += 1;
      row[1] += count(usage.cacheReadTokens);
      row[2] += count(usage.inputTokens);
      row[3] += count(usage.outputTokens);
    };
    for (const end of ends) {
      text += zstdDecompressSync(view.subarray(start, end)).toString('utf8');
      start = end;
      // Commit only whole lines: a frame that ends inside a line waits for
      // the next frame and is read again from its start later.
      if (!text.endsWith('\n')) continue;
      for (const line of text.split('\n')) {
        if (line.includes('"session/end-seed"')) {
          const record = JSON.parse(line);
          if (record.type === 'session/end-seed' && record.data?.inherited === true) {
            pending = {}; pendingCut = true;
          }
          continue;
        }
        if (!line.includes('"assistant/message"')) continue;
        const record = JSON.parse(line);
        const usage = record.data?.usage;
        if (record.type !== 'assistant/message' || !usage || typeof usage !== 'object' || !Number.isFinite(record.time)) continue;
        const day = localDay(record.time);
        if (day < sinceDay) continue;
        const source = record.data.message?.source;
        const model = typeof source?.model === 'string' && source.model
          ? source.model.replace(/[\x00-\x1f\x7f]/g, '').slice(0, 64) : 'unknown';
        add(pending, day, model, usage);
      }
      text = '';
      if (pendingCut) { result.days = {}; result.cut = true; pendingCut = false; }
      for (const [day, models] of Object.entries(pending))
        for (const [model, row] of Object.entries(models)) {
          const target = (result.days[day] ??= {})[model] ??= [0, 0, 0, 0];
          for (let i = 0; i < 4; i++) target[i] += row[i];
        }
      pending = {};
      result.offset = file.offset + end;
      if (Date.now() > deadline) break;
    }
    result.done = result.offset === size;
    return result;
  } catch (error) {
    return {...result, error: error?.code ? 'read' : 'format'};
  } finally {
    if (fd !== undefined) closeSync(fd);
  }
}

let input = '';
for await (const chunk of process.stdin) input += chunk;
const request = JSON.parse(input);
const deadline = Date.now() + request.max_ms;
let budget = request.max_bytes;
const results = [];
for (const file of request.files) {
  if (budget <= 0 || Date.now() > deadline) break;
  const result = readLog(file, request.since_day, budget, deadline);
  budget -= result.read;
  delete result.read;
  results.push(result);
}
process.stdout.write(JSON.stringify({results}));
