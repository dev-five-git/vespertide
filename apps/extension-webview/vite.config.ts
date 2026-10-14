import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import { DevupUI } from '@devup-ui/vite-plugin';
import { resolve } from 'path';

export default defineConfig({
  plugins: [react(), DevupUI()],
  build: {
    outDir: resolve(__dirname, '../vscode/dist/webview'),
    emptyOutDir: true,
    rollupOptions: {
      input: {
        index: resolve(__dirname, 'index.html'),
        'ai-agent': resolve(__dirname, 'ai-agent.html'),
      },
      output: {
        // Stable filenames — required for webview URI mapping
        entryFileNames: 'assets/[name].js',
        chunkFileNames: 'assets/[name].js',
        assetFileNames: 'assets/[name].[ext]',
      },
    },
  },
  base: './',
});
