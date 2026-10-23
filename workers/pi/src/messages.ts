import type { AgentSession } from '@earendil-works/pi-coding-agent';
import type { ChatMessage } from '../../../packages/contracts/src/index.js';

type ContentBlock = { type?: string; text?: string; thinking?: string };
type AnyMessage = {
  role?: string;
  content?: string | ContentBlock[];
  stopReason?: string;
  errorMessage?: string;
};

function textFromContent(content: string | ContentBlock[] | undefined): string {
  if (typeof content === 'string') return content;
  if (!Array.isArray(content)) return '';
  let text = '';
  for (const block of content) {
    if (block.type === 'text' && typeof block.text === 'string') text += block.text;
  }
  return text;
}

function reasoningFromContent(content: string | ContentBlock[] | undefined): string {
  if (!Array.isArray(content)) return '';
  let reasoning = '';
  for (const block of content) {
    if (block.type === 'thinking' && typeof block.thinking === 'string') reasoning += block.thinking;
  }
  return reasoning;
}

export function extractText(message: unknown): string {
  if (!message || typeof message !== 'object') return '';
  return textFromContent((message as AnyMessage).content);
}

export function extractReasoning(message: unknown): string {
  if (!message || typeof message !== 'object') return '';
  return reasoningFromContent((message as AnyMessage).content);
}

function assistantStatus(message: AnyMessage): ChatMessage['status'] {
  if (message.stopReason === 'error') return 'failed';
  if (message.stopReason === 'aborted') return 'cancelled';
  if (message.stopReason === 'pending') return 'generating';
  return 'completed';
}

function toChatMessage(message: AnyMessage, id: string): ChatMessage | undefined {
  if (message.role === 'user') {
    return { id, role: 'user', text: extractText(message), status: 'completed' };
  }
  if (message.role === 'assistant') {
    const reasoning = reasoningFromContent(message.content);
    return {
      id,
      role: 'assistant',
      text: extractText(message),
      ...(reasoning ? { reasoning } : {}),
      status: assistantStatus(message),
      ...(message.errorMessage ? { error: message.errorMessage } : {}),
    };
  }
  return undefined;
}





export function readSessionMessages(session: AgentSession): ChatMessage[] {
  const projection = session.sessionManager.buildSessionProjection();
  const messages: ChatMessage[] = [];
  for (const projected of projection.entries) {
    const entryId = projected.sourceEntry.id;
    projected.messages.forEach((message, index) => {
      const id = projected.messages.length > 1 ? `${entryId}:${index}` : entryId;
      const chat = toChatMessage(message as AnyMessage, id);
      if (chat) messages.push(chat);
    });
  }
  return messages;
}

export interface LastTurn {
  generationId: string | null;
  userText: string;
  userEntryId: string | null;
  userParentId: string | null;
  state: 'failed' | 'cancelled';
  assistantText: string;
  error?: string;
}

export interface ProjectedTurn {
  userText: string;
  userEntryId: string;
  userParentId: string | null;
  lastAssistant?: ChatMessage;
}


export function inspectLastTurn(session: AgentSession): ProjectedTurn | undefined {
  const projection = session.sessionManager.buildSessionProjection();
  let user: ProjectedTurn | undefined;
  for (const projected of projection.entries) {
    projected.messages.forEach((message, index) => {
      const chat = toChatMessage(
        message as AnyMessage,
        projected.messages.length > 1 ? `${projected.sourceEntry.id}:${index}` : projected.sourceEntry.id,
      );
      if (!chat) return;
      if (chat.role === 'user') {
        user = {
          userText: chat.text,
          userEntryId: projected.sourceEntry.id,
          userParentId: projected.sourceEntry.parentId,
        };
      } else if (chat.role === 'assistant' && user) {
        user.lastAssistant = chat;
      }
    });
  }
  return user;
}


export function lastRetryableTurn(session: AgentSession): LastTurn | undefined {
  const turn = inspectLastTurn(session);
  if (!turn) return undefined;
  const assistant = turn.lastAssistant;
  if (!assistant || assistant.status === 'failed' || assistant.status === 'cancelled') {
    const state = assistant?.status === 'cancelled' ? 'cancelled' : 'failed';
    return {
      generationId: null,
      userText: turn.userText,
      userEntryId: turn.userEntryId,
      userParentId: turn.userParentId,
      state,
      assistantText: assistant?.text ?? '',
      ...(assistant?.error
        ? { error: assistant.error }
        : !assistant
          ? { error: 'The previous generation did not complete' }
          : {}),
    };
  }
  return undefined;
}
