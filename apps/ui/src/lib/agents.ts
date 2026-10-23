import type { DelegationTask } from '../../../../packages/contracts/src/chat';
import { activeTheme, companionColor } from './theme.svelte';

const NAMES = ['Scout', 'Sage', 'Pip', 'Nova', 'Ember', 'Wren', 'Comet', 'Echo', 'Juno', 'Fable', 'Milo', 'Orbit', 'Tilly', 'Bolt', 'Fern', 'Ziggy'];
const COLORS = ['#82c7f0', '#8fe3a9', '#f59bb8', '#c4a2ff', '#ffab6e', '#7cded4', '#c9e871', '#f0c86a', '#9db4ff', '#ff8f8f', '#66d9c2', '#d9a2ff'];
export const COMPANION_COLORS = ['#82c7f0', '#8fe3a9', '#f59bb8', '#c4a2ff', '#ffab6e', '#7cded4', '#f0c86a', '#ff8f8f'];

function hash(value: string): number {
  let seed = 2166136261;
  for (let index = 0; index < value.length; index += 1) {
    seed ^= value.charCodeAt(index);
    seed = Math.imul(seed, 16777619);
  }
  return seed >>> 0;
}

function pick(list: string[], seed: number, taken: Set<string>): string {
  const start = seed % list.length;
  for (let offset = 0; offset < list.length; offset += 1) {
    const value = list[(start + offset) % list.length];
    if (!taken.has(value)) return value;
  }
  return list[start];
}

export interface AgentIdentity { name: string; color: string }

export function agentIdentities(tasks: DelegationTask[]): Map<string, AgentIdentity> {
  const ordered = [...tasks].sort((a, b) =>
    a.createdAt === b.createdAt ? (a.id < b.id ? -1 : 1) : a.createdAt < b.createdAt ? -1 : 1);
  const names = new Set<string>();
  const colors = new Set<string>([activeTheme().accent.toLowerCase(), companionColor().toLowerCase()]);
  const map = new Map<string, AgentIdentity>();
  for (const task of ordered) {
    const seed = hash(task.id);
    const given = task.agentName?.trim();
    const name = given && !names.has(given) ? given : pick(NAMES, seed, names);
    const color = pick(COLORS, seed, colors);
    names.add(name);
    colors.add(color);
    map.set(task.id, { name, color });
  }
  return map;
}

export function agentIdentity(task: DelegationTask, siblings: DelegationTask[] = []): AgentIdentity {
  const list = siblings.some(item => item.id === task.id) ? siblings : [task];
  return agentIdentities(list).get(task.id)!;
}
