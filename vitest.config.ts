import { defineConfig } from 'vitest/config';
import vue from '@vitejs/plugin-vue';
import { renderFontPlugin } from './scripts/render-font-plugin';

export default defineConfig({
  plugins: [vue(), renderFontPlugin()],
  test: {
    environment: 'jsdom',
    include: ['src/**/*.test.ts'],
    restoreMocks: true,
    clearMocks: true,
  },
});
