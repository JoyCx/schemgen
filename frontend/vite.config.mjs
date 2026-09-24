import { resolve } from 'node:path'
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

const root = import.meta.dirname

export default defineConfig({
  plugins: [react()],
  server: {
    port: 5173,
    proxy: {
      '/api': {
        target: 'http://localhost:3001',
        changeOrigin: true,
      },
    },
  },
  build: {
    outDir: 'dist',
    emptyOutDir: true,
    rollupOptions: {
      // shader-check.html compiles the de-light shader against a real WebGL
      // context. Listing it here ships it in dist, so it also works against
      // the production server, not only the dev server.
      input: {
        main: resolve(root, 'index.html'),
        shaderCheck: resolve(root, 'shader-check.html'),
      },
      output: {
        // three.js is most of the bundle and changes far less often than the
        // app, so it gets its own long-lived chunk.
        manualChunks: { three: ['three'] },
      },
    },
    // three.js alone is ~530 kB minified; warn only above that.
    chunkSizeWarningLimit: 600,
  },
})
