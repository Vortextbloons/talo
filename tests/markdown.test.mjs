import { test } from 'node:test';
import assert from 'node:assert/strict';
import { parseMarkdown, safeHref } from '../apps/ui/src/lib/markdown.ts';

function inlineText(nodes) {
  return nodes.map(node => node.kind === 'text' ? node.text : inlineText(node.children ?? [])).join('');
}

test('markdown renders emphasis, code and links', () => {
  const blocks = parseMarkdown('Some **bold**, *italic*, ~~gone~~, `code` and a [link](https://example.com).');
  assert.equal(blocks.length, 1);
  assert.equal(blocks[0].kind, 'paragraph');
  const kinds = blocks[0].children.map(node => node.kind);
  assert.ok(kinds.includes('strong'));
  assert.ok(kinds.includes('em'));
  assert.ok(kinds.includes('del'));
  assert.ok(kinds.includes('code'));
  assert.ok(kinds.includes('link'));
});

test('markdown keeps block structure for headings, code, quotes and rules', () => {
  const blocks = parseMarkdown('## Plan\n\n```ts\nconst x = 1;\n```\n\n> aside\n\n---');
  assert.deepEqual(blocks.map(block => block.kind), ['heading', 'code', 'quote', 'rule']);
  assert.equal(blocks[0].depth, 2);
  assert.equal(blocks[1].language, 'ts');
  assert.equal(blocks[1].text, 'const x = 1;');
});

test('markdown supports nested lists and task lists', () => {
  const [list] = parseMarkdown('- [x] done\n- [ ] todo\n  - nested');
  assert.equal(list.kind, 'list');
  assert.equal(list.items[0].task, true);
  assert.equal(list.items[0].checked, true);
  assert.equal(list.items[1].checked, false);
  assert.equal(list.items[1].blocks[1].kind, 'list');
});

test('markdown supports aligned GFM tables', () => {
  const [table] = parseMarkdown('| a | b |\n| :-- | --: |\n| 1 | 2 |');
  assert.equal(table.kind, 'table');
  assert.deepEqual(table.align, ['left', 'right']);
  assert.equal(table.header.length, 2);
  assert.equal(inlineText(table.rows[0][1]), '2');
});

test('markdown never turns unsafe links into anchors', () => {
  assert.equal(safeHref('javascript:alert(1)'), '');
  assert.equal(safeHref('data:text/html,x'), '');
  assert.equal(safeHref('/relative'), '');
  assert.equal(safeHref('https://example.com'), 'https://example.com');
  assert.equal(safeHref('mailto:hi@example.com'), 'mailto:hi@example.com');
  assert.equal(safeHref('#anchor'), '#anchor');
  const [paragraph] = parseMarkdown('[click](javascript:alert(1))');
  assert.equal(paragraph.children.some(node => node.kind === 'link'), false);
  assert.equal(inlineText(paragraph.children), 'click');
});

test('raw HTML stays inert text and partial streams do not throw', () => {
  assert.deepEqual(parseMarkdown('<img src=x onerror=alert(1)>'), [{ kind: 'raw', text: '<img src=x onerror=alert(1)>' }]);
  for (const partial of ['', ' ', '#', '**bold', '```', '| a |', '[t](', '- ', '![a](', '`', '~~~']) {
    assert.doesNotThrow(() => parseMarkdown(partial));
  }
});
