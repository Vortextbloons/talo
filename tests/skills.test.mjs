import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import os from 'node:os';
import { mkdtemp, mkdir, realpath, rm, writeFile } from 'node:fs/promises';
import { startWorker } from './helpers/worker.mjs';
import { startMockProvider } from './helpers/mock-provider.mjs';
import { fileTools } from '../dist/workers/pi/src/files.js';

async function approvedSkill(data, id, name, description) {
  const file = path.join(data, 'skills', id, 'SKILL.md');
  await mkdir(path.dirname(file), { recursive: true });
  await writeFile(file, `---\nname: ${name}\ndescription: "${description}"\n---\n\n## Steps\n\n1. Open the folder.\n`, 'utf8');
  return { name, description, path: file };
}

function systemText(messages) {
  return messages
    .filter(message => message.role === 'system')
    .map(message => Array.isArray(message.content) ? message.content.map(part => part.text ?? '').join('') : String(message.content ?? ''))
    .join('\n');
}

test('only the skills offered to a conversation are advertised, and newly approved ones join the next turn', { timeout: 45000 }, async () => {
  const data = await mkdtemp(path.join(os.tmpdir(), 'talo-skills-'));
  const provider = await startMockProvider();
  const worker = startWorker(data, { idleMs: 2000 });
  try {
    await worker.waitReady();
    const first = await approvedSkill(data, 'skill-1', 'organize-statements', 'Organize downloaded statements into folders.');
    const later = await approvedSkill(data, 'skill-2', 'file-receipts', 'File receipts by month.');
    await worker.request('open', 'session.open', {
      conversationId: 'skill-chat', provider: 'custom', model: 'mock-model',
      apiKey: provider.state.key, baseUrl: provider.url, sessionPath: null,
      skills: [first],
    });
    await worker.request('prompt', 'session.prompt', {
      conversationId: 'skill-chat', generationId: 'gen-1',
      text: 'Organize my statements', capabilities: ['files'], skills: [first],
    });
    await worker.waitGeneration('skill-chat', 'gen-1');
    const advertised = systemText(provider.state.requests.at(-1).body.messages);
    assert.match(advertised, /organize-statements/);
    assert.match(advertised, /Organize downloaded statements into folders\./);
    assert.ok(advertised.includes(first.path), 'the skill file is reachable at the advertised location');
    assert.doesNotMatch(advertised, /file-receipts/, 'a skill the host did not offer stays hidden');

    await worker.request('prompt', 'session.prompt', {
      conversationId: 'skill-chat', generationId: 'gen-2',
      text: 'Now file the receipts', capabilities: ['files'], skills: [first, later],
    });
    await worker.waitGeneration('skill-chat', 'gen-2');
    const refreshed = systemText(provider.state.requests.at(-1).body.messages);
    assert.match(refreshed, /organize-statements/);
    assert.match(refreshed, /file-receipts/, 'a skill approved mid-conversation is available without reopening it');
  } finally {
    await worker.kill();
    await provider.close();
    await rm(data, { recursive: true, force: true });
  }
});

test('approved skill files are readable without a permission card and never writable', async () => {
  const data = await mkdtemp(path.join(os.tmpdir(), 'talo-skills-files-'));
  try {
    const root = path.join(data, 'skills');
    const skill = await approvedSkill(data, 'skill-1', 'organize-statements', 'Organize statements.');
    let authorizations = 0;
    const seen = [];
    const cwd = path.join(data, 'agent-workspace');
    await mkdir(cwd, { recursive: true });
    const tools = fileTools(cwd, async () => { authorizations += 1; return false; }, {
      readRoots: [root],
      onSkillRead: file => seen.push(file),
    });
    const read = tools.find(tool => tool.name === 'read');
    const write = tools.find(tool => tool.name === 'write');

    const result = await read.execute('r1', { path: skill.path }, undefined, undefined, {});
    assert.match(result.content[0].text, /Open the folder/);
    assert.equal(authorizations, 0, 'reading an approved skill never asks for file access');
    assert.deepEqual(seen, [await realpath(skill.path)]);

    await assert.rejects(
      write.execute('w1', { path: skill.path, content: 'overwritten' }, undefined, undefined, {}),
      /review/i,
    );
    await writeFile(path.join(cwd, 'notes.md'), 'private', 'utf8');
    await assert.rejects(
      read.execute('r2', { path: path.join(cwd, 'notes.md') }, undefined, undefined, {}),
      /denied/i,
      'files outside the skill directory still need approval',
    );
  } finally {
    await rm(data, { recursive: true, force: true });
  }
});
