import { fileURLToPath, URL } from 'node:url'
import { svelte } from '@sveltejs/vite-plugin-svelte'
import tailwindcss from '@tailwindcss/vite'
import { defineConfig } from 'vite'

export default defineConfig({
  plugins: [tailwindcss(), svelte()],
  resolve: {
    alias: {
      $lib: fileURLToPath(new URL('./src/lib', import.meta.url)),
    },
  },
  worker: {
    format: 'es',
  },
  server: {
    // En développement, l'API Rust tourne sur le port 3000.
    proxy: {
      // ws : collaboration en direct sur le canevas.
      '/api': { target: 'http://127.0.0.1:3000', xfwd: true, ws: true },
    },
  },
})
