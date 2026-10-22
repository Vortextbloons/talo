import { mount } from 'svelte';
import App from './App.svelte';
import '../../../packages/design/tokens.css';
mount(App, { target: document.getElementById('app')! });
