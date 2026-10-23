import { readFile } from 'node:fs/promises';

const missing = 'Saved history is missing or unavailable. The conversation has been preserved.';
const damaged = 'Saved history is damaged. The original file has been preserved.';
const unreadable = 'Saved history could not be read';
const invalid = 'Saved history has an invalid message';

type Block = { type?: string; text?: string; thinking?: string };
type Entry = {
  type?: string;
  id?: string;
  message?: { role?: string; content?: string | Block[]; stopReason?: string };
};

function fail(message: string): never {
  process.stderr.write(message);
  process.exit(1);
}

const file = process.argv[2];
if (!file) fail(missing);

let raw: string;
try {
  raw = await readFile(file, 'utf8');
} catch (error) {
  const code = (error as NodeJS.ErrnoException).code;
  fail(code === 'ENOENT' ? missing : unreadable);
}

const messages = [];
let header = false;
for (const line of raw.split(/\r?\n/u)) {
  if (!line.trim()) continue;
  let entry: Entry;
  try {
    entry = JSON.parse(line) as Entry;
  } catch {
    fail(damaged);
  }
  if (entry.type === 'session') header = true;
  if (entry.type !== 'message') continue;
  const message = entry.message;
  const role = message?.role ?? '';
  if (role !== 'user' && role !== 'assistant') continue;
  let text = '';
  let reasoning = '';
  const content = message?.content;
  if (typeof content === 'string') text = content;
  if (Array.isArray(content)) {
    for (const block of content) {
      if (block.type === 'text') text += block.text ?? '';
      if (block.type === 'thinking') reasoning += block.thinking ?? '';
    }
  }
  if (typeof entry.id !== 'string' || !entry.id) fail(invalid);
  const stop = message?.stopReason;
  const status = stop === 'error' ? 'failed' : stop === 'aborted' ? 'cancelled' : 'completed';
  messages.push({
    id: entry.id,
    role,
    text,
    reasoning: reasoning || null,
    status,
    generationId: null,
    error: null,
  });
}
if (!header) fail(damaged);
process.stdout.write(JSON.stringify(messages));
