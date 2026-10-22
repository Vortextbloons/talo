import { Lexer } from 'marked';

// Model output is untrusted text. Marked only tokenizes here; MessageBody renders the tokens as
// Svelte elements, so no raw HTML ever reaches the webview. Raw HTML tokens therefore become
// literal text instead of markup.

export type Align = 'left' | 'center' | 'right' | null;

export type Inline =
  | { kind: 'text'; text: string }
  | { kind: 'strong'; children: Inline[] }
  | { kind: 'em'; children: Inline[] }
  | { kind: 'del'; children: Inline[] }
  | { kind: 'code'; text: string }
  | { kind: 'break' }
  | { kind: 'link'; href: string; title: string; children: Inline[] }
  | { kind: 'image'; href: string; alt: string; title: string };

export interface ListItem {
  task: boolean;
  checked: boolean;
  blocks: Block[];
}

export type Block =
  | { kind: 'paragraph'; children: Inline[] }
  | { kind: 'heading'; depth: number; children: Inline[] }
  | { kind: 'code'; language: string; text: string }
  | { kind: 'quote'; blocks: Block[] }
  | { kind: 'list'; ordered: boolean; start: number; loose: boolean; items: ListItem[] }
  | { kind: 'table'; align: Align[]; header: Inline[][]; rows: Inline[][][] }
  | { kind: 'rule' }
  | { kind: 'raw'; text: string };

/* The lexer's token union mixes specific shapes with a generic fallback, so read it loosely. */
interface RawToken {
  type: string;
  text?: string;
  tokens?: RawToken[];
  href?: string;
  title?: string | null;
  lang?: string;
  depth?: number;
  ordered?: boolean;
  start?: number | '';
  loose?: boolean;
  task?: boolean;
  checked?: boolean;
  align?: Align[];
  header?: RawToken[];
  rows?: RawToken[][];
  items?: RawToken[];
}

const SAFE_SCHEME = /^(https?|mailto)$/i;
const HAS_SCHEME = /^([a-z][a-z0-9+.-]*):/i;

/** Only http(s), mailto and in-page anchors are linkable; everything else stays plain text. */
export function safeHref(href: string | null | undefined): string {
  const value = (href ?? '').trim();
  if (!value) return '';
  if (value.startsWith('#')) return value;
  const scheme = HAS_SCHEME.exec(value);
  if (!scheme) return '';
  return SAFE_SCHEME.test(scheme[1]) ? value : '';
}

function pushText(nodes: Inline[], text: string): void {
  if (!text) return;
  const last = nodes[nodes.length - 1];
  if (last && last.kind === 'text') last.text += text;
  else nodes.push({ kind: 'text', text });
}

function foldChildren(parent: 'strong' | 'em' | 'del', children: Inline[]): Inline {
  return { kind: parent, children };
}

function inlineNodes(tokens: RawToken[] | undefined, nodes: Inline[] = []): Inline[] {
  for (const token of tokens ?? []) {
    switch (token.type) {
      case 'text':
        inlineNodes(token.tokens, nodes);
        if (!token.tokens) pushText(nodes, token.text ?? '');
        break;
      case 'escape':
        pushText(nodes, token.text ?? '');
        break;
      case 'strong':
      case 'em':
      case 'del': {
        const kind = token.type;
        nodes.push(foldChildren(kind, inlineNodes(token.tokens)));
        break;
      }
      case 'codespan':
        nodes.push({ kind: 'code', text: token.text ?? '' });
        break;
      case 'br':
        nodes.push({ kind: 'break' });
        break;
      case 'link': {
        const href = safeHref(token.href);
        const children = inlineNodes(token.tokens);
        if (href) nodes.push({ kind: 'link', href, title: token.title ?? '', children });
        else nodes.push(...children);
        break;
      }
      case 'image': {
        const alt = token.text ?? '';
        const href = safeHref(token.href);
        // The webview CSP blocks remote images, so an image is offered as a link to itself.
        if (href) nodes.push({ kind: 'image', href, alt: alt || href, title: token.title ?? '' });
        else if (alt) pushText(nodes, alt);
        break;
      }
      case 'html':
        // Shown as text; never parsed as markup.
        pushText(nodes, token.text ?? '');
        break;
      case 'checkbox':
      case 'space':
      case 'def':
        break;
      default:
        inlineNodes(token.tokens, nodes);
        break;
    }
  }
  return nodes;
}

function blockList(tokens: RawToken[] | undefined): Block[] {
  const blocks: Block[] = [];
  for (const token of tokens ?? []) {
    switch (token.type) {
      case 'paragraph':
        blocks.push({ kind: 'paragraph', children: inlineNodes(token.tokens) });
        break;
      case 'text':
        // Tight list items and table-ish text arrive as block-level text tokens.
        blocks.push({ kind: 'paragraph', children: inlineNodes(token.tokens) });
        break;
      case 'heading':
        blocks.push({
          kind: 'heading',
          depth: Math.min(6, Math.max(1, token.depth ?? 1)),
          children: inlineNodes(token.tokens),
        });
        break;
      case 'code':
        blocks.push({ kind: 'code', language: (token.lang ?? '').trim(), text: (token.text ?? '').replace(/\n$/, '') });
        break;
      case 'blockquote':
        blocks.push({ kind: 'quote', blocks: blockList(token.tokens) });
        break;
      case 'list':
        blocks.push({
          kind: 'list',
          ordered: token.ordered === true,
          start: typeof token.start === 'number' ? token.start : 1,
          loose: token.loose === true,
          items: (token.items ?? []).map(item => ({
            task: item.task === true,
            checked: item.checked === true,
            blocks: blockList(item.tokens),
          })),
        });
        break;
      case 'table': {
        const header = (token.header ?? []).map(cell => inlineNodes(cell.tokens));
        const rows = (token.rows ?? []).map(row => row.map(cell => inlineNodes(cell.tokens)));
        const align = (token.align ?? []).map(value => value ?? null);
        blocks.push({ kind: 'table', align, header, rows });
        break;
      }
      case 'hr':
        blocks.push({ kind: 'rule' });
        break;
      case 'html':
        blocks.push({ kind: 'raw', text: token.text ?? '' });
        break;
      case 'space':
      case 'def':
        break;
      default:
        if (token.tokens?.length) blocks.push(...blockList(token.tokens));
        else if (token.text) blocks.push({ kind: 'paragraph', children: [{ kind: 'text', text: token.text }] });
        break;
    }
  }
  return blocks;
}

/** Tokenizes markdown (GFM: tables, task lists, strikethrough, autolinks) into a small safe tree. */
export function parseMarkdown(source: string): Block[] {
  const text = source.replace(/\r\n?/g, '\n');
  if (!text.trim()) return [];
  const tokens = Lexer.lex(text, { gfm: true, breaks: true }) as unknown as RawToken[];
  return blockList(tokens);
}
