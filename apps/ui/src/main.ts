import { mount } from 'svelte';
import App from './App.svelte';
import '../../../packages/design/tokens.css';
import 'katex/dist/katex.min.css';
import { initTheme } from './lib/theme.svelte';

initTheme();
mount(App, { target: document.getElementById('app')! });
