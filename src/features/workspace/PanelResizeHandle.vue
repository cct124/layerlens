<script lang="ts">
// Reka 的鼠标注册器是窗口级的；边界交点可能同时激活两轴，取消应覆盖同次手势。
let cancellingResize = false;
</script>

<script setup lang="ts">
import { nextTick, onBeforeUnmount, onMounted, ref } from 'vue';
import { SplitterResizeHandle } from 'reka-ui';
import { SPLITTER_SIZE } from './panelLayout';

defineProps<{
  id: string;
  direction: 'horizontal' | 'vertical';
  label: string;
  valueText: string;
}>();
const emit = defineEmits<{ start: []; commit: []; cancel: []; reset: [] }>();
const root = ref<HTMLElement | null>(null);
let active = false;
let disposed = false;

function dragging(value: boolean) {
  if (value) {
    active = true;
    root.value?.focus();
    emit('start');
  } else if (active) {
    active = false;
    if (cancellingResize) emit('cancel');
    else emit('commit');
  }
}
function cancel() {
  if (!active) return;
  cancellingResize = true;
  // Reka 2.10 的公共接口没有取消手势：沿其标准 mouseup 路径结束注册器和全局光标，
  // 再让父组件通过公开 resize() 恢复尺寸；不改库内部状态，也不重挂载面板内容。
  try {
    window.dispatchEvent(new MouseEvent('mouseup', { clientX: -1, clientY: -1 }));
  } finally {
    cancellingResize = false;
  }
}
function escape(event: KeyboardEvent) {
  if (active && event.key === 'Escape' && !event.isComposing) {
    event.preventDefault();
    event.stopImmediatePropagation();
    cancel();
  }
}
function keydown(event: KeyboardEvent) {
  event.stopPropagation();
  if (active || event.isComposing || event.altKey || event.ctrlKey || event.metaKey) {
    event.preventDefault();
    return;
  }
  if (['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown', 'Home', 'End'].includes(event.key)) {
    emit('start');
    // Reka 的键盘监听在同一目标上执行，等待其布局更新后再保存一次用户意图。
    void nextTick(() => {
      if (!disposed) emit('commit');
    });
  }
}
function mouseDown(event: MouseEvent) {
  if (event.button !== 0 || event.buttons !== 1) event.stopPropagation();
}
function visibility() {
  if (document.hidden) cancel();
}
onMounted(() => {
  window.addEventListener('keydown', escape, true);
  window.addEventListener('blur', cancel);
  window.addEventListener('pointercancel', cancel, true);
  window.addEventListener('touchcancel', cancel, true);
  document.addEventListener('visibilitychange', visibility);
});
onBeforeUnmount(() => {
  disposed = true;
  cancel();
  window.removeEventListener('keydown', escape, true);
  window.removeEventListener('blur', cancel);
  window.removeEventListener('pointercancel', cancel, true);
  window.removeEventListener('touchcancel', cancel, true);
  document.removeEventListener('visibilitychange', visibility);
});
defineExpose({ cancel });
</script>

<template>
  <SplitterResizeHandle
    :id="id"
    as-child
    :hit-area-margins="{ fine: 0, coarse: 0 }"
    @dragging="dragging"
  >
    <div
      ref="root"
      class="panel-resize-handle"
      :style="{ flexBasis: `${SPLITTER_SIZE}px` }"
      :aria-label="label"
      :aria-orientation="direction === 'horizontal' ? 'vertical' : 'horizontal'"
      :aria-valuetext="valueText"
      :title="`${label} · 拖动或方向键调整 · 双击恢复默认`"
      @mousedown="mouseDown"
      @keydown="keydown"
      @dblclick.prevent.stop="emit('reset')"
      @contextmenu.prevent
    ></div>
  </SplitterResizeHandle>
</template>

<style scoped>
.panel-resize-handle {
  flex-grow: 0;
  flex-shrink: 0;
  background: transparent;
  touch-action: none;
}
.panel-resize-handle[data-orientation='horizontal'] {
  cursor: col-resize;
}
.panel-resize-handle[data-orientation='vertical'] {
  cursor: row-resize;
}
</style>
