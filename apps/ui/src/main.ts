import { mount } from 'svelte';
import App from './App.svelte';
import '../../../packages/design/tokens.css';
import 'katex/dist/katex.min.css';
mount(App, { target: document.getElementById('app')! });
