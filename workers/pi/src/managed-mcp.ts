import type { JSONRPCMessage, Transport } from '@modelcontextprotocol/client';
import type { ToolDefinition } from '@earendil-works/pi-coding-agent';
import type { ToolCategory } from '../../../packages/contracts/src/tools.js';
import { sanitizeError } from './providers.js';

export interface ManagedMcpGroup {
  id: 'talo-research' | 'talo-browser' | 'talo-agent' | 'talo-delegate';
  category: ToolCategory;
  description: string;
  tools: ToolDefinition[];
  timeoutMs: number;
  maxResultBytes: number;
  resources: string[];
}

/** A host-owned MCP endpoint over an in-process channel. It forwards to the
 * existing service boundary; it never launches a second browser/research server.
 * Each instance belongs to one Pi session, including that session's model. */
export class ManagedMcpTransport implements Transport {
  onmessage?: Transport['onmessage'];
  onclose?: Transport['onclose'];
  onerror?: Transport['onerror'];
  private closed = false;
  private pending = new Map<string | number, AbortController>();
  constructor(private group: ManagedMcpGroup) {}
  async start() { if (this.closed) throw new Error('Managed MCP endpoint is closed.'); }
  async close() {
    if (this.closed) return;
    this.closed = true;
    for (const controller of this.pending.values()) controller.abort();
    this.pending.clear();
    this.onclose?.();
  }
  async send(message: JSONRPCMessage) {
    if (this.closed) throw new Error('Managed MCP endpoint is closed.');
    if (!('method' in message)) return;
    const params = message.params as any;
    if (message.method === 'notifications/cancelled') {
      this.pending.get(params?.requestId)?.abort(); return;
    }
    if (!('id' in message)) return;
    const id = message.id;
    const reply = (result: any) => { if (!this.closed) this.onmessage?.({ jsonrpc: '2.0', id, result }); };
    if (message.method === 'initialize') {
      reply({ protocolVersion: params.protocolVersion, capabilities: { tools: {} }, serverInfo: { name: this.group.id, version: '1.0.0' } }); return;
    }
    if (message.method === 'ping') { reply({}); return; }
    if (message.method === 'tools/list') {
      reply({ tools: this.group.tools.map(tool => ({ name: tool.name, description: tool.description, inputSchema: tool.parameters })) }); return;
    }
    if (message.method !== 'tools/call') {
      this.onmessage?.({ jsonrpc: '2.0', id, error: { code: -32601, message: 'Unsupported managed MCP operation.' } }); return;
    }
    const tool = this.group.tools.find(tool => tool.name === params?.name);
    if (!tool) { this.onmessage?.({ jsonrpc: '2.0', id, error: { code: -32602, message: 'Unknown managed tool.' } }); return; }
    const controller = new AbortController(); this.pending.set(id, controller);
    // Do not await the operation in send(): cancellation notifications must keep
    // flowing on the same channel while an approval or browser action is pending.
    void tool.execute(String(id), params.arguments ?? {}, controller.signal, undefined, undefined as any).then(
      result => reply({ content: result.content }),
      error => reply({ isError: true, content: [{ type: 'text', text: controller.signal.aborted ? 'Tool operation cancelled.' : sanitizeError(error).slice(0,500) }] }),
    ).finally(() => this.pending.delete(id));
  }
}
