import { Type } from 'typebox';
import type { ToolDefinition } from '@earendil-works/pi-coding-agent';
import type { WebTransport } from './web.js';

export const DELEGATION_TOOL = 'delegate';

const optionalId = Type.Optional(Type.String({ maxLength: 100 }));
const parameters = Type.Object({
  operation: Type.Union([
    Type.Literal('start'),
    Type.Literal('list'),
    Type.Literal('cancel'),
    Type.Literal('cancel_all'),
  ]),
  title: Type.Optional(Type.String({ maxLength: 120 })),
  brief: Type.Optional(Type.String({ maxLength: 4000 })),
  depth: Type.Optional(Type.Union([Type.Literal('flash'), Type.Literal('smart')])),
  taskId: optionalId,
}, { additionalProperties: false });

const description = [
  'Delegate one bounded, independent subtask to a child task in this conversation.',
  'Operations:',
  'start — create a child with a short title and a self-contained brief. Returns a stable task ID immediately; the child keeps running independently. Give every child the full context it needs, because it does not receive this transcript. Pass depth "flash" for quick lookups and "smart" for hard analysis; omit it to follow the performance tier.',
  'list — inspect this conversation’s delegated tasks and their current states.',
  'cancel — stop one delegated task without touching siblings, this chat, or other chats.',
  'cancel_all — stop every active delegated task owned by this conversation.',
  'Use delegation for parallel research or analysis. Complete simple work directly. Children cannot delegate further or change the app.',
].join(' ');

export function createDelegationTools(transport: WebTransport, conversationId: string): ToolDefinition[] {
  return [{
    name: DELEGATION_TOOL,
    label: 'Delegated tasks',
    description,
    parameters,
    async execute(_id, input: any, signal) {
      const result = await transport.request(
        'delegation.call',
        { ...input, conversationId },
        signal,
        30_000,
      );
      return {
        content: [{ type: 'text' as const, text: JSON.stringify(result).slice(0, 16_000) }],
        details: { operation: String(input?.operation ?? '') },
      };
    },
  }];
}
