import { execFileSync } from 'node:child_process';
import { readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import ts from 'typescript';
import { parse as parseSvelte } from 'svelte/compiler';

const root = path.resolve(import.meta.dirname, '..');
const sourceExtensions = new Set([
  '.cjs', '.css', '.cts', '.html', '.js', '.jsx', '.mjs', '.mts',
  '.ps1', '.rs', '.sql', '.svelte', '.ts', '.tsx',
]);

function collectTypeScriptComments(source, fileName) {
  const scriptKind = fileName.endsWith('.tsx') || fileName.endsWith('.jsx')
    ? ts.ScriptKind.TSX
    : fileName.endsWith('.js') || fileName.endsWith('.jsx') || fileName.endsWith('.mjs') || fileName.endsWith('.cjs')
      ? ts.ScriptKind.JS
      : ts.ScriptKind.TS;
  const tree = ts.createSourceFile(fileName, source, ts.ScriptTarget.Latest, true, scriptKind);
  const comments = new Map();

  function visit(node) {
    const ranges = [
      ...ts.getLeadingCommentRanges(source, node.getFullStart()) ?? [],
      ...ts.getTrailingCommentRanges(source, node.end) ?? [],
    ];
    for (const range of ranges) comments.set(range.pos, range.end);
    ts.forEachChild(node, visit);
  }

  visit(tree);
  return [...comments].map(([start, end]) => ({ start, end }));
}

function collectSvelteComments(source) {
  const tree = parseSvelte(source);
  const comments = new Map();

  function addComments(ranges, offset = 0) {
    for (const { start, end } of ranges) comments.set(start + offset, end + offset);
  }

  function visitJavaScript(value) {
    if (!value || typeof value !== 'object') return;
    if (Array.isArray(value)) {
      for (const item of value) visitJavaScript(item);
      return;
    }
    if (
      ['Line', 'Block'].includes(value.type)
      && Number.isInteger(value.start)
      && Number.isInteger(value.end)
    ) {
      comments.set(value.start, value.end);
    }
    for (const [key, child] of Object.entries(value)) {
      if (key !== 'parent') visitJavaScript(child);
    }
  }

  function visit(value) {
    if (!value || typeof value !== 'object') return;
    if (Array.isArray(value)) {
      for (const item of value) visit(item);
      return;
    }
    if (['Comment', 'CSSComment'].includes(value.type)
      && Number.isInteger(value.start) && Number.isInteger(value.end)) {
      comments.set(value.start, value.end);
    }
    if (value.type === 'Script' || value.type === 'Style') {
      const contentStart = source.indexOf('>', value.start) + 1;
      const tag = value.type.toLowerCase();
      const closingTag = source.toLowerCase().indexOf(`</${tag}`, contentStart);
      if (contentStart === 0 || closingTag < 0 || closingTag >= value.end) {
        throw new Error(`Unable to locate Svelte ${tag} content`);
      }
      const content = source.slice(contentStart, closingTag);
      addComments(
        value.type === 'Script'
          ? collectTypeScriptComments(content, 'inline.ts')
          : collectCssComments(content),
        contentStart,
      );
      return;
    }
    for (const [key, child] of Object.entries(value)) {
      if (key === 'parent' || key === 'content') continue;
      if (['expression', 'test', 'context', 'key', 'declaration', 'guard', 'pattern'].includes(key)) {
        visitJavaScript(child);
      } else {
        visit(child);
      }
    }
  }

  visit(tree);
  return [...comments].map(([start, end]) => ({ start, end }));
}

function collectCssComments(source) {
  const comments = [];
  let index = 0;

  while (index < source.length) {
    if (source[index] === '"' || source[index] === "'") {
      const quote = source[index++];
      while (index < source.length) {
        if (source[index] === '\\') index += 2;
        else if (source[index++] === quote) break;
      }
    } else if (source.startsWith('/*', index)) {
      const end = source.indexOf('*/', index + 2);
      if (end < 0) throw new Error('Unterminated CSS block comment');
      comments.push({ start: index, end: end + 2 });
      index = end + 2;
    } else {
      index++;
    }
  }

  return comments;
}

function skipQuoted(source, start, quote, doubledQuote = false) {
  let index = start + 1;
  while (index < source.length) {
    if (source[index] === '\\') {
      index += 2;
    } else if (source[index] === quote) {
      if (doubledQuote && source[index + 1] === quote) index += 2;
      else return index + 1;
    } else {
      index++;
    }
  }
  throw new Error(`Unterminated ${quote} string`);
}

function collectSqlComments(source) {
  const comments = [];
  let index = 0;

  while (index < source.length) {
    const char = source[index];
    if (char === "'" || char === '"' || char === '`') {
      index = skipQuoted(source, index, char, true);
    } else if (char === '[') {
      const end = source.indexOf(']', index + 1);
      if (end < 0) throw new Error('Unterminated SQL bracket identifier');
      index = end + 1;
    } else if (source.startsWith('--', index)) {
      const start = index;
      index += 2;
      while (index < source.length && source[index] !== '\r' && source[index] !== '\n') index++;
      comments.push({ start, end: index });
    } else if (source.startsWith('/*', index)) {
      const start = index;
      index += 2;
      let depth = 1;
      while (index < source.length && depth > 0) {
        if (source.startsWith('/*', index)) {
          depth++;
          index += 2;
        } else if (source.startsWith('*/', index)) {
          depth--;
          index += 2;
        } else {
          index++;
        }
      }
      if (depth > 0) throw new Error('Unterminated SQL block comment');
      comments.push({ start, end: index });
    } else if (char === '$') {
      const delimiter = source.slice(index).match(/^\$[A-Za-z_][A-Za-z\d_]*\$|^\$\$/)?.[0];
      if (delimiter) {
        const end = source.indexOf(delimiter, index + delimiter.length);
        if (end < 0) throw new Error('Unterminated SQL dollar-quoted string');
        index = end + delimiter.length;
      } else {
        index++;
      }
    } else {
      index++;
    }
  }

  return comments;
}

function collectRustComments(source) {
  const comments = [];
  let index = 0;

  while (index < source.length) {
    const rawStart = source.slice(index).match(/^(?:b|c)?r(#+)?"/);
    if (rawStart) {
      const delimiter = `"${rawStart[1] ?? ''}`;
      const end = source.indexOf(delimiter, index + rawStart[0].length);
      if (end < 0) throw new Error('Unterminated Rust raw string');
      index = end + delimiter.length;
    } else if (source[index] === '"' || source[index] === "'") {
      const quote = source[index];
      if (quote === '"') {
        index = skipQuoted(source, index, quote);
      } else {
        const charLiteral = source.slice(index).match(/^'(?:\\(?:[nrt0\\'" ]|x[0-9A-Fa-f]{2}|u\{[0-9A-Fa-f_]+\})|[^'\\\r\n])'/);
        index += charLiteral ? charLiteral[0].length : 1;
      }
    } else if (source.startsWith('//', index)) {
      const start = index;
      index += 2;
      while (index < source.length && source[index] !== '\r' && source[index] !== '\n') index++;
      comments.push({ start, end: index });
    } else if (source.startsWith('/*', index)) {
      const start = index;
      index += 2;
      let depth = 1;
      while (index < source.length && depth > 0) {
        if (source.startsWith('/*', index)) {
          depth++;
          index += 2;
        } else if (source.startsWith('*/', index)) {
          depth--;
          index += 2;
        } else {
          index++;
        }
      }
      if (depth > 0) throw new Error('Unterminated Rust block comment');
      comments.push({ start, end: index });
    } else {
      index++;
    }
  }

  return comments;
}

function collectPowerShellComments(source) {
  const comments = [];
  let index = 0;

  while (index < source.length) {
    if (source.startsWith("<#", index)) {
      const start = index;
      const end = source.indexOf('#>', index + 2);
      if (end < 0) throw new Error('Unterminated PowerShell block comment');
      comments.push({ start, end: end + 2 });
      index = end + 2;
    } else if (
      source[index] === '@'
      && (source[index + 1] === "'" || source[index + 1] === '"')
      && (source[index + 2] === '\n' || source[index + 2] === '\r')
    ) {
      const terminator = `${source[index + 1]}@`;
      const start = index;
      let lineStart = source.indexOf('\n', index) + 1;
      let end = -1;
      while (lineStart > 0 && lineStart <= source.length) {
        if (source.startsWith(terminator, lineStart)) {
          end = lineStart + 2;
          break;
        }
        const next = source.indexOf('\n', lineStart);
        if (next < 0) break;
        lineStart = next + 1;
      }
      if (end < 0) throw new Error('Unterminated PowerShell here-string');
      index = end;
      if (source[index] === '\r') index++;
      if (source[index] === '\n') index++;
      if (source[start] !== '@') throw new Error('Invalid PowerShell here-string');
    } else if (source[index] === "'" || source[index] === '"') {
      const quote = source[index++];
      while (index < source.length) {
        if (quote === '"' && source[index] === '`') index += 2;
        else if (quote === "'" && source[index] === quote && source[index + 1] === quote) index += 2;
        else if (source[index++] === quote) break;
      }
    } else if (source[index] === '#') {
      const start = index++;
      while (index < source.length && source[index] !== '\r' && source[index] !== '\n') index++;
      comments.push({ start, end: index });
    } else {
      index++;
    }
  }

  return comments;
}

function collectHtmlComments(source) {
  const comments = [];
  let index = 0;

  while (index < source.length) {
    if (source.startsWith('<!--', index)) {
      const end = source.indexOf('-->', index + 4);
      if (end < 0) throw new Error('Unterminated HTML comment');
      comments.push({ start: index, end: end + 3 });
      index = end + 3;
    } else if (/^<(script|style)\b/i.test(source.slice(index))) {
      const tag = source.slice(index).match(/^<(script|style)\b/i)[1].toLowerCase();
      let openEnd = index;
      let quote = null;
      while (openEnd < source.length) {
        const char = source[openEnd++];
        if (quote) {
          if (char === quote) quote = null;
        } else if (char === '"' || char === "'") {
          quote = char;
        } else if (char === '>') {
          break;
        }
      }
      const closeStart = source.toLowerCase().indexOf(`</${tag}`, openEnd);
      if (closeStart < 0) throw new Error(`Unterminated HTML <${tag}> element`);
      const content = source.slice(openEnd, closeStart);
      const nested = tag === 'style'
        ? collectCssComments(content)
        : collectTypeScriptComments(content, 'inline.js');
      comments.push(...nested.map(({ start, end }) => ({ start: start + openEnd, end: end + openEnd })));
      index = closeStart;
    } else if (source[index] === '<') {
      let quote = null;
      index++;
      while (index < source.length) {
        const char = source[index++];
        if (quote) {
          if (char === quote) quote = null;
        } else if (char === '"' || char === "'") {
          quote = char;
        } else if (char === '>') {
          break;
        }
      }
    } else {
      index++;
    }
  }

  return comments;
}

export function commentRanges(source, fileName) {
  const extension = path.extname(fileName).toLowerCase();
  switch (extension) {
    case '.cjs':
    case '.js':
    case '.jsx':
    case '.mjs':
    case '.ts':
    case '.tsx':
    case '.cts':
    case '.mts':
      return collectTypeScriptComments(source, fileName);
    case '.svelte':
      return collectSvelteComments(source);
    case '.rs':
      return collectRustComments(source);
    case '.sql':
      return collectSqlComments(source);
    case '.css':
      return collectCssComments(source);
    case '.html':
      return collectHtmlComments(source);
    case '.ps1':
      return collectPowerShellComments(source);
    default:
      throw new Error(`Unsupported source file type: ${fileName}`);
  }
}

export function stripComments(source, comments) {
  if (!comments.length) return source;
  let result = '';
  let previousEnd = 0;

  for (const { start, end } of comments.sort((left, right) => left.start - right.start)) {
    if (start < previousEnd || end <= start) continue;
    result += source.slice(previousEnd, start);
    const comment = source.slice(start, end);
    result += comment.includes('\n') || comment.includes('\r')
      ? comment.replace(/[^\r\n]/g, '')
      : ' ';
    previousEnd = end;
  }

  return (result + source.slice(previousEnd))
    .replace(/[ \t]+(?=\r?$)/gm, '')
    .replace(/(?:\r?\n)*$/, '\n');
}

function isExcluded(file) {
  return file.startsWith('vendor/')
    || file.startsWith('apps/desktop/gen/')
    || /(^|\/)(?:target|dist|build|node_modules)\//.test(file);
}

async function main(args) {
  const write = args.includes('--write');
  const check = args.includes('--check');
  const unknownArgs = args.filter(arg => !['--write', '--check', '--help'].includes(arg));
  if (unknownArgs.length) throw new Error(`Unknown option(s): ${unknownArgs.join(', ')}`);
  if (args.includes('--help')) {
    console.log('Usage: bun scripts/strip-comments.mjs [--write | --check]');
    console.log('Dry-run is the default. --write modifies tracked source files.');
    return;
  }
  if (write && check) throw new Error('Choose either --write or --check.');

  const files = execFileSync('git', ['ls-files', '-z'], { cwd: root, encoding: 'utf8' })
    .split('\0')
    .filter(file => file && !isExcluded(file) && sourceExtensions.has(path.extname(file).toLowerCase()));
  const changed = [];

  for (const file of files) {
    const fullPath = path.join(root, file);
    const source = await readFile(fullPath, 'utf8');
    const comments = commentRanges(source, file);
    if (!comments.length) continue;
    const output = stripComments(source, comments);
    if (output === source) continue;
    changed.push({ file, fullPath, output, count: comments.length });
    if (write) await writeFile(fullPath, output, 'utf8');
  }

  for (const { file, count } of changed) {
    console.log(`${write ? 'Updated' : 'Would update'} ${file} (${count} comment${count === 1 ? '' : 's'})`);
  }
  console.log(`${write ? 'Updated' : 'Found'} ${changed.length} tracked source file(s) with comments.`);
  if (check && changed.length) process.exitCode = 1;
}

if (process.argv[1] && path.resolve(process.argv[1]) === import.meta.filename) {
  main(process.argv.slice(2)).catch(error => {
    console.error(error);
    process.exitCode = 1;
  });
}
