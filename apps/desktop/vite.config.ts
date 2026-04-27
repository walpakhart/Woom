import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';

const host = process.env.TAURI_DEV_HOST;

export default defineConfig(async () => ({
  plugins: [sveltekit()],

  // Vite options tailored for Tauri development
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? { protocol: 'ws', host, port: 1421 }
      : undefined,
    watch: {
      ignored: ['**/src-tauri/**']
    }
  },
  // Vitest config — pure-TS unit tests for the lib/ utilities
  // (cost math, token formatting, context-window helpers, etc).
  // No DOM, no Svelte components — those would need a separate
  // jsdom environment and a Svelte test plugin; today we only test
  // the parts that don't depend on either.
  test: {
    include: ['src/**/*.{test,spec}.ts'],
    environment: 'node',
    globals: false
  }
}));
