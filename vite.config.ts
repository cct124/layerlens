import { defineConfig } from 'vite';
import vue from '@vitejs/plugin-vue';
import { renderFontPlugin } from './scripts/render-font-plugin';

export default defineConfig(({ mode }) => ({
  plugins: [vue(), renderFontPlugin()],
  clearScreen: false,
  server: {
    host: '127.0.0.1',
    port: 15420,
    strictPort: true,
    watch: { ignored: ['**/src-tauri/**', '**/crates/**', '**/target/**', '**/.local/**'] },
  },
  build: {
    target: 'es2022',
    ...(mode === 'render-lab'
      ? { outDir: 'dist-render-lab', rollupOptions: { input: ['index.html', 'render-lab.html'] } }
      : {}),
  },
}));
