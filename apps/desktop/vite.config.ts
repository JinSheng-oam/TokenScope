import { defineConfig } from 'vitest/config';
import { svelte } from '@sveltejs/vite-plugin-svelte';

export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  server: { host: '127.0.0.1', port: 1420, strictPort: true },
  test: { environment: 'node', include: ['src/**/*.test.ts'] },
});
