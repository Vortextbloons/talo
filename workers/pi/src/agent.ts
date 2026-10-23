import { Type } from 'typebox';
import type { ToolDefinition } from '@earendil-works/pi-coding-agent';
import type { WebTransport } from './web.js';

/** The single tool exported by the `talo-agent` server. Operations are host-validated. */
export const AGENT_TOOL = 'agent';

const optionalId = (maxLength: number) => Type.Optional(Type.String({ maxLength }));
const option = Type.Object({
  id: optionalId(40),
  label: Type.String({ maxLength: 120 }),
  detail: Type.Optional(Type.String({ maxLength: 200 })),
}, { additionalProperties: false });
const question = Type.Object({
  id: optionalId(40),
  question: Type.String({ maxLength: 300 }),
  options: Type.Array(option, { minItems: 2, maxItems: 5 }),
  recommended: optionalId(40),
  other: Type.Optional(Type.Boolean()),
}, { additionalProperties: false });

const parameters = Type.Object({
  operation: Type.Union([
    Type.Literal('state'),
    Type.Literal('space_list'),
    Type.Literal('space_create'),
    Type.Literal('space_update'),
    Type.Literal('task_list'),
    Type.Literal('task_assign'),
    Type.Literal('skill_list'),
    Type.Literal('skill_create'),
    Type.Literal('skill_update'),
    Type.Literal('ask'),
  ]),
  spaceId: optionalId(100),
  skillId: optionalId(100),
  description: Type.Optional(Type.String({ maxLength: 1024 })),
  name: Type.Optional(Type.String({ maxLength: 60 })),
  instructions: Type.Optional(Type.String({ maxLength: 12000 })),
  pinned: Type.Optional(Type.Array(Type.String({ maxLength: 400 }), { maxItems: 8 })),
  memoryScope: Type.Optional(Type.Union([
    Type.Literal('inherit'),
    Type.Literal('workspace'),
    Type.Literal('off'),
  ])),
  capabilities: Type.Optional(Type.Array(Type.Union([
    Type.Literal('web'),
    Type.Literal('browser'),
    Type.Literal('files'),
    Type.Literal('google'),
  ]), { maxItems: 4 })),
  workspaceId: optionalId(100),
  title: Type.Optional(Type.String({ maxLength: 200 })),
  detail: Type.Optional(Type.String({ maxLength: 2000 })),
  why: Type.Optional(Type.String({ maxLength: 300 })),
  due: Type.Optional(Type.String({ maxLength: 40 })),
  header: Type.Optional(Type.String({ maxLength: 120 })),
  questions: Type.Optional(Type.Array(question, { minItems: 1, maxItems: 3 })),
}, { additionalProperties: false });

const description = [
  'Shape Talo around the user: draft a workspace, hand them a task, or ask them a structured question.',
  'Operations:',
  'state — your remaining daily budget, open assignments, waiting questions and workspace drafts.',
  'space_list — every accepted workspace with its bundle (instructions, pinned notes, memory scope, capabilities).',
  'space_create — draft a workspace bundle: name, instructions, up to 8 pinned notes, memoryScope (inherit|workspace|off), capabilities (web|browser|files|google). It appears in the user’s Inbox as a draft; the user accepts or dismisses it.',
  'space_update — change a bundle you own (a workspace the user created with you, or one they let you edit). User-owned workspaces are read-only to you: propose the change in an assignment instead.',
  'task_assign — a to-do for the USER (not for you). Include why it matters now. Use it for follow-ups only a person can do; never assign busywork or a task you could complete yourself.',
  'task_list — open and recent assignments.',
  'skill_list — approved skills with their name, description, workspace scope, revision and SKILL.md path.',
  'skill_create — propose a saved skill for a repeatable workflow you just completed successfully: title, description (what it does and when to use it), and instructions in Markdown with the sections How to verify the outcome, Required tools and prerequisites, Steps, naming conventions, Decisions that need the user, and Known mistakes to avoid. Use workspaceId to keep it for one workspace, or omit it to make it available in every workspace. Never include passwords, account numbers, file contents or cookies. The user approves, edits or dismisses it in the Inbox; it is not active before that.',
  'skill_update — propose a corrected revision of an approved skill (skillId plus the changed title, description or instructions). The approved version stays active until the user approves the proposal. If you are a delegated child, report the suggested correction in your result instead; your parent proposes it.',
  'ask — a blocking question card with 1–3 questions, each with 2–5 options and your recommended option. Use it only when the answer changes what you do next and cannot be inferred; never to ask permission. If the user does not answer within five minutes the recommended options are used and you are told they were defaults.',
  'The user may cap how much you create; respect the error and ask instead of retrying.',
].join(' ');

export function createAgentTools(transport: WebTransport, conversationId: string): ToolDefinition[] {
  return [{
    name: AGENT_TOOL,
    label: 'Talo workspace and tasks',
    description,
    parameters,
    async execute(_id, input: any, signal) {
      // A question card can block for up to five minutes; the transport budget covers it.
      const result = await transport.request(
        'agent.call',
        { ...input, conversationId },
        signal,
        330_000,
      );
      const text = JSON.stringify(result);
      return {
        content: [{ type: 'text' as const, text: text.slice(0, 16_000) }],
        details: { operation: String(input?.operation ?? '') },
      };
    },
  }];
}
