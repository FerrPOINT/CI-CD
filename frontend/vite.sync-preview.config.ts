import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
import { fileURLToPath, URL } from 'node:url'

export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: { alias: { '@': fileURLToPath(new URL('./src', import.meta.url)) } },
  server: { host: '127.0.0.1', port: 22912, strictPort: true },
  build: {
    outDir: 'dist-sync-preview',
    rollupOptions: { input: fileURLToPath(new URL('./sync-preview.html', import.meta.url)) },
  },
})
