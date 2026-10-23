import test from 'node:test';
import assert from 'node:assert/strict';
import { commentRanges, stripComments } from '../scripts/strip-comments.mjs';

function strip(source, fileName) {
  return stripComments(source, commentRanges(source, fileName));
}

test('JavaScript and TypeScript comments are removed without changing comment-like strings or regexes', () => {
  const source = [
    '// leading',
    'const text = "https://example.test/*not a comment*/";',
    'const expression = /\\/\\* not a comment \\*\\//;',
    'const template = `text // ${1 /* actual comment */}`;',
    '/* block */ const value = 1;',
  ].join('\n');
  const output = strip(source, 'example.ts');

  assert.match(output, /https:\/\/example\.test\/\*not a comment\*\//);
  assert.ok(output.includes('const expression = /\\/\\* not a comment \\*\\//;'));
  assert.doesNotMatch(output, /leading|actual comment|block/);
});

test('Svelte comments are removed from markup, scripts, and styles', () => {
  const source = [
    '<!-- markup comment -->',
    '<script>const value = 1; // script comment</script>',
    '<div>{value /* expression comment */}</div>',
    '<style>/* style comment */ .card { color: red; }</style>',
  ].join('\n');
  const output = strip(source, 'Example.svelte');

  assert.doesNotMatch(output, /markup comment|script comment|expression comment|style comment/);
  assert.match(output, /<div>\{value\s+\}<\/div>/);
  assert.match(output, /\.card\s*\{\s*color:\s*red;\s*\}/);
});

test('Rust comments are removed while nested comments and raw strings are parsed safely', () => {
  const source = [
    "let lifetime: &'static str = r###\"// not a comment\"###;",
    '/* outer /* inner */ comment */ let value = 1;',
    '// line comment',
  ].join('\n');
  const output = strip(source, 'example.rs');

  assert.match(output, /r###"\/\/ not a comment"###/);
  assert.match(output, /&'static str/);
  assert.doesNotMatch(output, /outer|inner|line comment/);
});

test('SQL and PowerShell comments are removed without changing quoted values', () => {
  const sql = strip("SELECT '-- text', '/* text */'; -- remove\n/* remove */ SELECT 1;", 'schema.sql');
  const powershell = strip([
    '$text = "# not a comment"',
    "@'\n# here-string text\n'@",
    '# remove this',
    'Write-Output $text',
  ].join('\n'), 'script.ps1');

  assert.match(sql, /'-- text', '\/\* text \*\//);
  assert.doesNotMatch(sql, /remove/);
  assert.match(powershell, /# not a comment/);
  assert.match(powershell, /# here-string text/);
  assert.doesNotMatch(powershell, /remove this/);
});

test('HTML comments are removed while inline script and style comments are parsed by language', () => {
  const source = '<!-- html --> <script>const text = "// value"; // code</script><style>/* code */ .x{}</style>';
  const output = strip(source, 'index.html');

  assert.match(output, /"\/\/ value"/);
  assert.doesNotMatch(output, /html|code/);
});
