import { invoke, isTauri } from '@tauri-apps/api/core';

export type SurfaceVar =
  | '--bg' | '--paper' | '--soft' | '--sidebar' | '--text'
  | '--muted' | '--faint' | '--line' | '--line-strong' | '--black';

export interface Theme { id: string; name: string; accent: string; surfaces?: Partial<Record<SurfaceVar, string>> }

export const PRESETS: Theme[] = [
  { id: 'amber', name: 'Amber', accent: '#ffd45c' },
  { id: 'midnight', name: 'Midnight', accent: '#ece9e2', surfaces: {
    '--bg': '#000000', '--paper': '#0d0d0c', '--soft': '#1a1a19', '--sidebar': '#000000',
    '--text': '#f2f1ec', '--muted': '#a8a7a1', '--faint': '#737270',
    '--line': '#262624', '--line-strong': '#3a3a37', '--black': '#000000',
  } },
  { id: 'nord', name: 'Nord', accent: '#88c0d0', surfaces: {
    '--bg': '#2e3440', '--paper': '#3b4252', '--soft': '#434c5e', '--sidebar': '#272c36',
    '--text': '#eceff4', '--muted': '#aebacf', '--faint': '#7b88a1',
    '--line': '#3f4757', '--line-strong': '#4c566a', '--black': '#272c36',
  } },
  { id: 'dracula', name: 'Dracula', accent: '#bd93f9', surfaces: {
    '--bg': '#282a36', '--paper': '#343746', '--soft': '#44475a', '--sidebar': '#21222c',
    '--text': '#f8f8f2', '--muted': '#a3a7c4', '--faint': '#6272a4',
    '--line': '#3a3d4e', '--line-strong': '#4e5064', '--black': '#21222c',
  } },
  { id: 'solarized', name: 'Solarized', accent: '#268bd2', surfaces: {
    '--bg': '#002b36', '--paper': '#073642', '--soft': '#0b4553', '--sidebar': '#002029',
    '--text': '#eee8d5', '--muted': '#93a1a1', '--faint': '#586e75',
    '--line': '#0d4250', '--line-strong': '#1b5a6b', '--black': '#00202a',
  } },
  { id: 'catppuccin', name: 'Catppuccin', accent: '#b4befe', surfaces: {
    '--bg': '#181825', '--paper': '#1e1e2e', '--soft': '#313244', '--sidebar': '#11111b',
    '--text': '#cdd6f4', '--muted': '#a6adc8', '--faint': '#6c7086',
    '--line': '#313244', '--line-strong': '#45475a', '--black': '#11111b',
  } },
  { id: 'rosepine', name: 'Rosé Pine', accent: '#ebbcba', surfaces: {
    '--bg': '#191724', '--paper': '#1f1d2e', '--soft': '#26233a', '--sidebar': '#13111d',
    '--text': '#e0def4', '--muted': '#908caa', '--faint': '#6e6a86',
    '--line': '#26233a', '--line-strong': '#403d52', '--black': '#13111d',
  } },
  { id: 'gruvbox', name: 'Gruvbox', accent: '#fe8019', surfaces: {
    '--bg': '#1d2021', '--paper': '#282828', '--soft': '#3c3836', '--sidebar': '#191c1d',
    '--text': '#ebdbb2', '--muted': '#a89984', '--faint': '#7c6f64',
    '--line': '#3a3632', '--line-strong': '#504945', '--black': '#191c1d',
  } },
  { id: 'mint', name: 'Mint', accent: '#7fe0a8' },
  { id: 'aqua', name: 'Aqua', accent: '#7cded4' },
  { id: 'sky', name: 'Sky', accent: '#82c7f0' },
  { id: 'lilac', name: 'Lilac', accent: '#c4a2ff' },
  { id: 'rose', name: 'Rose', accent: '#ff9db8' },
  { id: 'tangerine', name: 'Tangerine', accent: '#ffab6e' },
  { id: 'lime', name: 'Lime', accent: '#c9e871' },
];

const DEFAULT_ID = PRESETS[0].id;
const ACTIVE_KEY = 'talo.theme';
const CUSTOM_KEY = 'talo.themes.custom';
const COMPANION_KEY = 'talo.companion.color';
const SURFACES: SurfaceVar[] = ['--bg', '--paper', '--soft', '--sidebar', '--text', '--muted', '--faint', '--line', '--line-strong', '--black'];

export const themes = $state({ activeId: DEFAULT_ID, custom: [] as Theme[] });
export const companion = $state({ color: '' });

function all(): Theme[] {
  return [...PRESETS, ...themes.custom];
}

function apply(theme: Theme): void {
  const root = document.documentElement.style;
  root.setProperty('--accent', theme.accent);
  for (const surface of SURFACES) {
    const value = theme.surfaces?.[surface];
    if (value) root.setProperty(surface, value);
    else root.removeProperty(surface);
  }
  if (isTauri()) void invoke('set_window_background', { color: theme.surfaces?.['--bg'] ?? '#121211' }).catch(() => {});
}

function applyCompanion(): void {
  const root = document.documentElement.style;
  if (companion.color) root.setProperty('--companion', companion.color);
  else root.removeProperty('--companion');
}

export function activeTheme(): Theme {
  return all().find(theme => theme.id === themes.activeId) ?? PRESETS[0];
}

export function companionColor(): string {
  return companion.color || activeTheme().accent;
}

export function setCompanionColor(color: string): void {
  const value = color.trim().toLowerCase();
  if (!value) return;
  companion.color = value;
  applyCompanion();
  try { localStorage.setItem(COMPANION_KEY, companion.color); } catch { /* storage can be unavailable */ }
}

export function resetCompanionColor(): void {
  companion.color = '';
  applyCompanion();
  try { localStorage.removeItem(COMPANION_KEY); } catch { /* storage can be unavailable */ }
}

export function selectTheme(id: string): void {
  const theme = all().find(item => item.id === id);
  if (!theme) return;
  themes.activeId = theme.id;
  apply(theme);
  try { localStorage.setItem(ACTIVE_KEY, theme.id); } catch { /* storage can be unavailable */ }
}

function themeId(): string {
  return `custom-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 6)}`;
}

function normalizeColor(value: unknown): string | undefined {
  if (typeof value !== 'string') return undefined;
  const text = value.trim().toLowerCase();
  const short = /^#([0-9a-f]{3})$/u.exec(text);
  if (short) return `#${short[1].split('').map(character => character + character).join('')}`;
  return /^#[0-9a-f]{6}$/u.test(text) ? text : undefined;
}

const SURFACE_KEYS = new Set<string>(SURFACES);

function normalizeSurfaces(value: unknown): Partial<Record<SurfaceVar, string>> | undefined {
  if (typeof value !== 'object' || value === null) return undefined;
  const surfaces: Partial<Record<SurfaceVar, string>> = {};
  for (const [key, color] of Object.entries(value)) {
    const normalized = normalizeColor(color);
    if (normalized && SURFACE_KEYS.has(key)) surfaces[key as SurfaceVar] = normalized;
  }
  return Object.keys(surfaces).length ? surfaces : undefined;
}

export function createTheme(name: string, accent: string, base?: Theme): Theme {
  const theme: Theme = {
    id: themeId(),
    name: name.trim().slice(0, 24) || 'My theme',
    accent: accent.toLowerCase(),
    ...(base?.surfaces ? { surfaces: { ...base.surfaces } } : {}),
  };
  themes.custom = [...themes.custom, theme];
  try { localStorage.setItem(CUSTOM_KEY, JSON.stringify(themes.custom)); } catch { /* storage can be unavailable */ }
  selectTheme(theme.id);
  return theme;
}

export function addTheme(draft: unknown): Theme {
  const source = (typeof draft === 'object' && draft !== null ? draft : {}) as Record<string, unknown>;
  const accent = normalizeColor(source.accent);
  if (!accent) throw new Error('That theme has no valid accent color.');
  const name = typeof source.name === 'string' ? source.name.replace(/\s+/gu, ' ').trim().slice(0, 24) : '';
  const surfaces = normalizeSurfaces(source.surfaces);
  const theme: Theme = {
    id: themeId(),
    name: name || 'Imported theme',
    accent,
    ...(surfaces ? { surfaces } : {}),
  };
  themes.custom = [...themes.custom, theme];
  try { localStorage.setItem(CUSTOM_KEY, JSON.stringify(themes.custom)); } catch { /* storage can be unavailable */ }
  selectTheme(theme.id);
  return theme;
}

export function deleteTheme(id: string): void {
  if (!themes.custom.some(theme => theme.id === id)) return;
  themes.custom = themes.custom.filter(theme => theme.id !== id);
  try { localStorage.setItem(CUSTOM_KEY, JSON.stringify(themes.custom)); } catch { /* storage can be unavailable */ }
  if (themes.activeId === id) selectTheme(DEFAULT_ID);
}

export function initTheme(): void {
  try {
    const stored = localStorage.getItem(CUSTOM_KEY);
    const parsed: unknown = stored ? JSON.parse(stored) : null;
    if (Array.isArray(parsed)) {
      themes.custom = parsed.filter((item: any): item is Theme =>
        typeof item?.id === 'string' && typeof item?.name === 'string' && typeof item?.accent === 'string');
    }
    const saved = localStorage.getItem(ACTIVE_KEY);
    if (saved && all().some(theme => theme.id === saved)) themes.activeId = saved;
    companion.color = localStorage.getItem(COMPANION_KEY) ?? '';
  } catch { /* storage can be unavailable */ }
  apply(activeTheme());
  applyCompanion();
}
