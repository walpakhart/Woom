import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';

const host = process.env.TAURI_DEV_HOST;

export default defineConfig(async () => ({
  plugins: [sveltekit()],

  // Force browser-side resolution for `svelte` and friends. Without this,
  // Vite 8 + the version of `@sveltejs/vite-plugin-svelte` we're on resolve
  // `import { onDestroy } from 'svelte'` to `svelte/src/index-server.js`,
  // which throws at runtime in the browser ("Cannot read properties of
  // undefined (reading 'r')") and leaves the WebView a black screen at
  // first mount. The `browser` condition forces the client-side entry,
  // matching how `pnpm dev` already resolves things via the Tauri WebView.
  resolve: {
    conditions: ['browser', 'module', 'import', 'default']
  },

  // Emit source maps in production builds so WebKit devtools can
  // resolve minified stack traces (Le/D/Ni/Ii/Xr/etc) back to the
  // original `.svelte` / `.ts` files. Cheap (`hidden-source-map`
  // keeps the .map files alongside the JS but doesn't reference
  // them inline, so production users don't ship `//# sourceMappingURL`
  // comments — devs can still load the maps manually).
  build: {
    sourcemap: 'inline'
  },

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
