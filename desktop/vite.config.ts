import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import tailwindcss from '@tailwindcss/vite';

export default defineConfig({
  plugins: [react(), tailwindcss()],
  server: {
    port: 3000,
    host: true,
    proxy: {
      '/api': {
        target: 'http://127.0.0.1:4096',
        changeOrigin: true,
        // A same-origin browser fetch sends no Origin header, and the daemon
        // only hands out its loopback session token to an explicit trusted
        // origin. Without this, the dev UI ends up with no token: every call
        // answers 401, the UI reports "daemon offline", and the tool list
        // collapses to the 71-entry static fallback while the daemon really
        // serves 212. Stamping the dev origin here keeps the daemon rule
        // strict (it still refuses unknown origins) while making dev work.
        configure: (proxy) => {
          proxy.on('proxyReq', (proxyReq) => {
            proxyReq.setHeader('origin', 'http://localhost:3000');
          });
        },
      },
    },
  },
});
