<script setup lang="ts">
import { computed } from 'vue';
import WorkspaceIcon from './WorkspaceIcon.vue';
import { useWindowControls } from './useWindowControls';

const props = defineProps<{ desktop: boolean; connected: boolean; picking: boolean }>();
const { maximized, pending, error, dismissError, run, startDragging } = useWindowControls(
  props.desktop,
);
const maximizeLabel = computed(() =>
  maximized.value === null ? '最大化 / 还原窗口' : maximized.value ? '还原窗口' : '最大化窗口',
);

function titlebarMouseDown(event: MouseEvent) {
  if (
    !props.desktop ||
    event.defaultPrevented ||
    event.button !== 0 ||
    event.buttons !== 1 ||
    !(event.target instanceof Element) ||
    !event.target.closest('[data-window-drag-region]')
  )
    return;
  event.preventDefault();
  // 原生拖动可能接管后续鼠标事件，按 Tauri 推荐在第二次按下时处理双击。
  // 自有标记避免与 data-tauri-drag-region 的内置监听重复执行。
  if (event.detail === 2) void run('toggleMaximize');
  else void startDragging();
}
</script>

<template>
  <header class="app-header" aria-label="LayerLens 标题栏" @mousedown="titlebarMouseDown">
    <div class="brand" data-window-drag-region><WorkspaceIcon name="brand" />LayerLens</div>
    <slot />
    <span v-if="picking" role="status" class="menu-status">选择文件中…</span>
    <span class="header-spacer" data-window-drag-region></span>
    <span class="connection-status" data-window-drag-region
      ><span class="status-dot" :class="{ ready: connected }"></span
      >{{ connected ? '工作区已连接' : desktop ? '未连接' : '浏览器预览' }}</span
    >
    <span class="readonly-label" data-window-drag-region
      ><WorkspaceIcon name="lock" />PSD 只读</span
    >
    <div v-if="desktop" class="window-controls" role="group" aria-label="窗口控制">
      <button
        type="button"
        aria-label="最小化窗口"
        title="最小化窗口"
        :disabled="!!pending"
        @click="run('minimize')"
      >
        <svg viewBox="0 0 12 12" aria-hidden="true"><path d="M1 6.5h10" /></svg>
      </button>
      <button
        type="button"
        :aria-label="maximizeLabel"
        :title="maximizeLabel"
        :disabled="!!pending"
        @click="run('toggleMaximize')"
      >
        <svg viewBox="0 0 12 12" aria-hidden="true">
          <path v-if="maximized" d="M3.5 3.5v-2h7v7h-2 M1.5 3.5h7v7h-7z" />
          <path v-else d="M1.5 1.5h9v9h-9z" />
        </svg>
      </button>
      <button
        type="button"
        class="window-close"
        aria-label="关闭窗口"
        title="关闭窗口 (Alt+F4)"
        :disabled="!!pending"
        @click="run('close')"
      >
        <svg viewBox="0 0 12 12" aria-hidden="true"><path d="m1 1 10 10M1 11 11 1" /></svg>
      </button>
    </div>
  </header>
  <div v-if="error" class="notice error-notice window-error" role="alert">
    <span>{{ error }}</span>
    <button aria-label="关闭窗口提示" @click="dismissError">×</button>
  </div>
</template>

<style scoped>
.window-controls {
  display: flex;
  align-self: stretch;
  flex: 0 0 auto;
  margin-left: 12px;
}
.window-controls button {
  width: 46px;
  border: 0;
  border-radius: 0;
  padding: 0;
  background: transparent;
  cursor: default;
}
.window-controls button:hover:not(:disabled) {
  background: #45474d;
}
.window-controls .window-close:hover:not(:disabled) {
  background: #c42b1c;
  color: #fff;
}
.window-controls svg {
  width: 12px;
  height: 12px;
  fill: none;
  stroke: currentColor;
  stroke-width: 1;
}
.window-error > span {
  flex: 1;
  overflow-wrap: anywhere;
}
</style>
