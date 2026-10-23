import type { DelegationActivity } from '../../../packages/contracts/src/chat.js';

const MAX_LABEL = 80;
const MAX_DETAIL = 200;

function text(value: unknown, max: number): string {
  if (typeof value !== 'string') return '';
  const flattened = value.replace(/[\u0000-\u001f\u007f]/gu, ' ').trim().replace(/\s+/gu, ' ');
  return flattened.length > max ? `${flattened.slice(0, max - 1)}…` : flattened;
}

function safeUrl(value: unknown): string {
  if (typeof value !== 'string') return '';
  try {
    const url = new URL(value);
    if (!['http:', 'https:'].includes(url.protocol)) return '';
    url.username = '';
    url.password = '';
    url.search = '';
    url.hash = '';
    return text(url.href, MAX_DETAIL);
  } catch {
    return '';
  }
}

function fileName(value: unknown): string {
  if (typeof value !== 'string') return '';
  const parts = value.replaceAll('\\', '/').split('/').filter(Boolean);
  return text(parts.slice(-2).join('/'), MAX_DETAIL);
}

function browserAction(value: unknown): string {
  return value === 'click' ? 'Clicking on the page'
    : value === 'type' ? 'Typing on the page'
      : value === 'select' ? 'Choosing an option'
        : value === 'press' ? 'Pressing a key'
          : value === 'scroll' ? 'Reading further down'
            : 'Interacting with the page';
}

export function describeTool(tool: string, args: any): { label: string; detail?: string } {
  const input = args && typeof args === 'object' ? args : {};
  let label = 'Working';
  let detail = '';
  switch (tool) {
    case 'web_search':
      label = 'Searching the web';
      detail = text(input.query, MAX_DETAIL);
      break;
    case 'web_fetch':
      label = 'Reading a webpage';
      detail = safeUrl(input.url);
      break;
    case 'web_crawl':
      label = 'Reading a website';
      detail = safeUrl(input.url);
      break;
    case 'web_screenshot':
      label = 'Capturing a webpage';
      detail = safeUrl(input.url);
      break;
    case 'browser_open':
      label = 'Opening its browser';
      detail = safeUrl(input.url);
      break;
    case 'browser_tabs':
      label = 'Switching browser tabs';
      detail = text(input.action, MAX_DETAIL);
      break;
    case 'browser_snapshot':
      label = 'Reading the page';
      break;
    case 'browser_interact':
      label = browserAction(input.action);
      detail = text(input.selector, MAX_DETAIL);
      break;
    case 'browser_ai':
      label = input.action === 'extract' ? 'Extracting details' : 'Examining the page';
      detail = text(input.instruction, MAX_DETAIL);
      break;
    case 'browser_screenshot':
      label = 'Taking a screenshot';
      break;
    case 'browser_download':
      label = 'Downloading a file';
      detail = safeUrl(input.url);
      break;
    case 'browser_handoff':
      label = 'Waiting for browser control';
      detail = text(input.action, MAX_DETAIL);
      break;
    case 'read':
      label = 'Reading a file';
      detail = fileName(input.path);
      break;
    case 'write':
      label = 'Writing a file';
      detail = fileName(input.path);
      break;
    case 'edit':
      label = 'Editing a file';
      detail = fileName(input.path);
      break;
    case 'agent':
      label = input.operation === 'ask' ? 'Asking you a question'
        : input.operation === 'schedule_save' ? 'Saving a scheduled task'
          : input.operation === 'schedule_pause' ? 'Updating a scheduled task'
            : input.operation === 'schedule_delete' ? 'Deleting a scheduled task'
          : input.operation === 'space_create' || input.operation === 'space_update' ? 'Preparing a workspace'
            : input.operation === 'schedule_list' ? 'Checking scheduled tasks'
              : 'Checking Talo';
      break;
    case 'tools': {
      const inner = text(input.tool, 100);
      if (input.operation === 'call' && inner && input.server !== 'talo-delegate') {
        const described = describeTool(inner, input.arguments ?? {});
        label = described.label;
        detail = described.detail ?? '';
      } else if (input.server === 'talo-delegate') {
        label = 'Delegating work';
        detail = inner;
      } else if (input.server) {
        label = 'Looking for a tool';
        detail = text(input.server, MAX_DETAIL);
      } else {
        label = 'Looking for a tool';
      }
      break;
    }
    case 'delegate':
      label = 'Delegating work';
      break;
    case 'mcp':
      label = 'Using an integration';
      detail = text(input.server, MAX_DETAIL);
      break;
    default:
      label = tool ? tool.replaceAll('_', ' ').replace(/^./u, value => value.toUpperCase()) : 'Working';
  }
  return detail ? { label: text(label, MAX_LABEL), detail } : { label: text(label, MAX_LABEL) };
}

export function activityFor(
  id: string,
  tool: string,
  args: unknown,
  state: DelegationActivity['state'],
): DelegationActivity {
  const described = describeTool(tool, args);
  return {
    id,
    tool: text(tool, 100),
    label: described.label,
    ...(described.detail ? { detail: described.detail } : {}),
    state,
  };
}
