import { invoke } from '@tauri-apps/api/core';
import { mount } from 'svelte';
import App from './App.svelte';
import { isTauriRuntime } from './lib/api';
import './app.css';

function dismissBootSplash() {
  const splash = document.getElementById('boot-splash');
  if (!splash) return;
  let removed = false;
  const cleanup = () => {
    if (removed) return;
    removed = true;
    splash.remove();
  };
  window.requestAnimationFrame(() => {
    splash.classList.add('is-hidden');
    splash.addEventListener('transitionend', cleanup, { once: true });
    window.setTimeout(cleanup, 320);
  });
}

function notifyWindowReady() {
  if (!isTauriRuntime()) return;
  void invoke('reveal_main_window').catch(() => {});
}

const target = document.getElementById('app');
if (!target) throw new Error('Missing app mount element');
mount(App, { target });
notifyWindowReady();
dismissBootSplash();
