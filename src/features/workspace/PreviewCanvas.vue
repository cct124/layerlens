<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, shallowRef, watch } from 'vue';
import { fitZoom, initialView, locateBounds, zoomAround } from './canvasView';
import type { CanvasView } from './canvasView';
import type { Bounds, SelectionBounds } from '../../shared/api/generated';
import {
  beginRegion,
  documentPoint,
  finishRegion,
  insideDocument,
  screenPoint,
  updateRegion,
} from './regionDraft';
import type { CanvasTool, RegionAction, RegionDraft, RegionHandle } from './regionDraft';
const props = defineProps<{
  width: number;
  height: number;
  url: string | null;
  name: string;
  view: CanvasView;
  bounds: Bounds | null;
  tool: CanvasTool;
  region: SelectionBounds | null;
  contextKey: string;
  disabled: boolean;
}>();
const emit = defineEmits<{
  'update:view': [view: CanvasView];
  imageError: [];
  region: [bounds: SelectionBounds, contextKey: string];
  clearSelection: [contextKey: string];
}>();
const viewport = ref<HTMLElement | null>(null);
const size = ref({ width: 0, height: 0 });
const effective = computed(() =>
  props.view.mode === 'fit'
    ? {
        ...initialView(),
        zoom: fitZoom(props.width, props.height, size.value.width, size.value.height),
      }
    : props.view,
);
let observer: ResizeObserver | undefined;
const drag = shallowRef<{
  id: number;
  button: 0 | 1;
  x: number;
  y: number;
  view: CanvasView;
} | null>(null);
const regionDrag = shallowRef<{
  id: number;
  x: number;
  y: number;
  contextKey: string;
  draft: RegionDraft;
} | null>(null);
const drawingNew = ref(false);
const displayedRegion = computed(() => regionDrag.value?.draft.bounds ?? props.region);
const handles: { id: RegionHandle; x: number; y: number; label: string }[] = [
  { id: 'nw', x: 0, y: 0, label: '左上角' },
  { id: 'n', x: 50, y: 0, label: '上边' },
  { id: 'ne', x: 100, y: 0, label: '右上角' },
  { id: 'e', x: 100, y: 50, label: '右边' },
  { id: 'se', x: 100, y: 100, label: '右下角' },
  { id: 's', x: 50, y: 100, label: '下边' },
  { id: 'sw', x: 0, y: 100, label: '左下角' },
  { id: 'w', x: 0, y: 50, label: '左边' },
];
function releasePointer(id: number | undefined) {
  if (id !== undefined && viewport.value?.hasPointerCapture(id))
    viewport.value.releasePointerCapture(id);
}
function cancel() {
  const id = regionDrag.value?.id ?? drag.value?.id;
  regionDrag.value = null;
  drag.value = null;
  drawingNew.value = false;
  releasePointer(id);
}
function escape(event: KeyboardEvent) {
  // 长按产生的重复事件不能把一次取消草稿升级为清空已提交范围。
  if (event.repeat || event.isComposing) return;
  const wasDragging = !!regionDrag.value || !!drag.value;
  cancel();
  if (!wasDragging && props.tool === 'region' && !props.disabled)
    emit('clearSelection', props.contextKey);
}
watch(
  () => [props.contextKey, props.tool, props.disabled, props.url, props.width, props.height],
  cancel,
  { flush: 'sync' },
);
watch(
  () => props.view,
  () => {
    if (regionDrag.value) cancel();
  },
  { flush: 'sync' },
);
onMounted(() => {
  observer = new ResizeObserver((entries) => {
    const entry = entries[0];
    if (entry) {
      cancel();
      size.value = { width: entry.contentRect.width, height: entry.contentRect.height };
    }
  });
  if (viewport.value) observer.observe(viewport.value);
  window.addEventListener('blur', cancel);
});
onUnmounted(() => {
  cancel();
  observer?.disconnect();
  window.removeEventListener('blur', cancel);
});
function setView(view: CanvasView) {
  cancel();
  emit('update:view', view);
}
function zoom(factor: number) {
  setView(zoomAround(effective.value, effective.value.zoom * factor));
}
function wheel(event: WheelEvent) {
  if (!viewport.value) return;
  const bounds = viewport.value.getBoundingClientRect();
  setView(
    zoomAround(
      effective.value,
      effective.value.zoom * Math.exp(-Math.max(-200, Math.min(200, event.deltaY)) * 0.002),
      event.clientX - bounds.left - bounds.width / 2,
      event.clientY - bounds.top - bounds.height / 2,
    ),
  );
}
function start(event: PointerEvent) {
  if (props.tool === 'region') return startRegion(event, 'create');
  if (event.button === 0 && !regionDrag.value) startPan(event, 0);
}
function startPan(event: PointerEvent, button: 0 | 1) {
  if (
    !viewport.value ||
    drag.value ||
    (regionDrag.value && regionDrag.value.id !== event.pointerId)
  )
    return;
  event.preventDefault();
  // 同一指针从左键草稿转为中键平移时保留捕获，避免释放捕获事件取消新手势。
  regionDrag.value = null;
  drawingNew.value = false;
  viewport.value.focus({ preventScroll: true });
  viewport.value.setPointerCapture(event.pointerId);
  drag.value = {
    id: event.pointerId,
    button,
    x: event.clientX,
    y: event.clientY,
    view: effective.value,
  };
}
function middleDown(event: PointerEvent) {
  if (event.button !== 1) return;
  // 在捕获阶段处理，选框内部与控制点也能发起中键平移。
  event.stopPropagation();
  startPan(event, 1);
}
function preventMiddleDefault(event: MouseEvent) {
  if (event.button === 1) event.preventDefault();
}
function cancelPointer(event: PointerEvent) {
  if (event.pointerId === drag.value?.id || event.pointerId === regionDrag.value?.id) cancel();
}
function move(event: PointerEvent) {
  const pan = drag.value;
  if (pan?.id === event.pointerId) {
    // 鼠标多键组合只在最后一个键松开时派发 pointerup，按 buttons 检查中键提前松开。
    if (pan.button === 1 && !(event.buttons & 4)) {
      cancel();
      return;
    }
    emit('update:view', {
      ...pan.view,
      mode: 'manual',
      x: pan.view.x + event.clientX - pan.x,
      y: pan.view.y + event.clientY - pan.y,
    });
    return;
  }
  const region = regionDrag.value;
  if (region?.id === event.pointerId) {
    // 左键仍按住时追加中键不会再次产生 pointerdown。
    if (event.buttons & 4) {
      startPan(event, 1);
      return;
    }
    regionDrag.value = {
      ...region,
      draft: updateRegion(region.draft, point(event), props.width, props.height),
    };
    return;
  }
}
function point(event: PointerEvent) {
  const rect = viewport.value!.getBoundingClientRect();
  return documentPoint(
    { x: event.clientX - rect.left, y: event.clientY - rect.top },
    {
      width: props.width,
      height: props.height,
      viewportWidth: rect.width,
      viewportHeight: rect.height,
      view: effective.value,
    },
  );
}
function startRegion(event: PointerEvent, action: RegionAction) {
  if (
    event.button !== 0 ||
    props.tool !== 'region' ||
    props.disabled ||
    !props.url ||
    !viewport.value ||
    regionDrag.value ||
    drag.value
  )
    return;
  const start = point(event);
  // 控制点可能伸出画布几像素；只有新建必须从文档内部开始。
  if (action === 'create' && !insideDocument(start, props.width, props.height)) return;
  viewport.value.focus({ preventScroll: true });
  viewport.value.setPointerCapture(event.pointerId);
  regionDrag.value = {
    id: event.pointerId,
    x: event.clientX,
    y: event.clientY,
    contextKey: props.contextKey,
    draft: beginRegion(action, start, props.region),
  };
}
function end(event: PointerEvent) {
  const region = regionDrag.value;
  if (region?.id === event.pointerId) {
    const draft = updateRegion(region.draft, point(event), props.width, props.height);
    const bounds = finishRegion(
      draft,
      Math.hypot(event.clientX - region.x, event.clientY - region.y),
      props.width,
      props.height,
    );
    cancel();
    if (bounds && !props.disabled && region.contextKey === props.contextKey)
      emit('region', bounds, region.contextKey);
  } else if (drag.value?.id === event.pointerId) cancel();
}
function boundaryStyle(bounds: SelectionBounds) {
  const position = screenPoint(bounds, {
    width: props.width,
    height: props.height,
    viewportWidth: 0,
    viewportHeight: 0,
    view: effective.value,
  });
  return {
    left: `calc(50% + ${position.x}px)`,
    top: `calc(50% + ${position.y}px)`,
    width: `${bounds.width * effective.value.zoom}px`,
    height: `${bounds.height * effective.value.zoom}px`,
  };
}
function locate() {
  if (props.bounds?.width && props.bounds.height)
    setView(
      locateBounds(props.width, props.height, props.bounds, size.value.width, size.value.height),
    );
}
defineExpose({ locate, cancel });
</script>
<template>
  <div class="preview-pane">
    <div class="canvas-controls" aria-label="画布视图">
      <span>{{ tool === 'region' ? '区域选择 · 松开确认，Esc 取消' : '保存时合成预览' }}</span>
      <div class="zoom-controls">
        <button
          v-if="tool === 'region' && region"
          :disabled="disabled"
          :aria-pressed="drawingNew"
          @click="
            cancel();
            drawingNew = true;
          "
        >
          重新框选
        </button>
        <button aria-label="缩小" @click="zoom(0.8)">−</button
        ><output aria-label="当前缩放">{{ (effective.zoom * 100).toFixed(0) }}%</output
        ><button aria-label="放大" @click="zoom(1.25)">+</button
        ><button @click="setView({ mode: 'manual', zoom: 1, x: 0, y: 0 })">100%</button
        ><button @click="setView(initialView())">适应窗口</button>
      </div>
    </div>
    <div
      ref="viewport"
      class="preview-viewport"
      :class="{ 'region-tool': tool === 'region', panning: !!drag }"
      aria-label="PSD 预览画布"
      tabindex="0"
      @wheel.prevent="wheel"
      @pointerdown.capture="middleDown"
      @mousedown="preventMiddleDefault"
      @auxclick="preventMiddleDefault"
      @pointerdown="start"
      @pointermove="move"
      @pointerup="end"
      @pointercancel="cancelPointer"
      @lostpointercapture="cancelPointer"
      @keydown.esc.prevent="escape"
      @keydown.left.prevent="setView({ ...effective, mode: 'manual', x: effective.x - 30 })"
      @keydown.right.prevent="setView({ ...effective, mode: 'manual', x: effective.x + 30 })"
      @keydown.up.prevent="setView({ ...effective, mode: 'manual', y: effective.y - 30 })"
      @keydown.down.prevent="setView({ ...effective, mode: 'manual', y: effective.y + 30 })"
    >
      <img
        v-if="url"
        :key="url"
        class="preview-image"
        :src="url"
        :alt="`${name} 保存时合成预览`"
        draggable="false"
        :width="width"
        :height="height"
        :style="{
          width: `${width}px`,
          height: `${height}px`,
          transform: `translate(-50%, -50%) translate(${effective.x}px, ${effective.y}px) scale(${effective.zoom})`,
        }"
        @error="emit('imageError')"
      />
      <div
        v-if="bounds && bounds.width > 0 && bounds.height > 0"
        class="layer-boundary"
        aria-label="选中图层几何边界"
        :style="boundaryStyle(bounds)"
      ></div>
      <div
        v-if="url && displayedRegion"
        class="region-boundary"
        :class="{
          'region-editable': tool === 'region' && !disabled && !drawingNew,
          'region-draft': !!regionDrag,
        }"
        aria-label="任务区域选框"
        :style="boundaryStyle(displayedRegion)"
        @pointerdown.stop.prevent="startRegion($event, 'move')"
      >
        <span class="region-size"
          >{{ Math.round(displayedRegion.width * 100) / 100 }} ×
          {{ Math.round(displayedRegion.height * 100) / 100 }} px</span
        >
        <template v-if="tool === 'region' && !disabled && !drawingNew">
          <span
            v-for="handle in handles"
            :key="handle.id"
            class="region-handle"
            :data-region-handle="handle.id"
            :title="`调整${handle.label}`"
            :style="{
              left: `${handle.x}%`,
              top: `${handle.y}%`,
              cursor: drag ? 'grabbing' : `${handle.id}-resize`,
            }"
            @pointerdown.stop.prevent="startRegion($event, handle.id)"
          ></span>
        </template>
      </div>
      <div v-if="!url" class="canvas-message"><slot /></div>
    </div>
    <div class="canvas-footer">
      <span>{{ width }} × {{ height }} 原稿像素</span
      ><span>{{
        tool === 'region'
          ? '左键框选／调整 · 中键平移 · Esc 取消拖动／空闲时清空'
          : '拖动平移 · 滚轮缩放 · 方向键移动'
      }}</span>
    </div>
  </div>
</template>

<style scoped>
.region-tool,
.region-tool:active {
  cursor: crosshair;
}
.region-boundary {
  position: absolute;
  z-index: 2;
  box-sizing: border-box;
  border: 1px solid #287149;
  box-shadow: 0 0 0 100000px #0006;
  pointer-events: none;
}
.region-editable {
  pointer-events: auto;
  cursor: move;
}
.region-draft {
  border-style: dashed;
}
.region-handle {
  position: absolute;
  width: 9px;
  height: 9px;
  box-sizing: border-box;
  background: #fff;
  border: 1px solid #287149;
  transform: translate(-50%, -50%);
}
.region-size {
  position: absolute;
  left: 0;
  bottom: calc(100% + 7px);
  padding: 2px 5px;
  background: #21432f;
  color: #fff;
  font-size: 11px;
  white-space: nowrap;
  pointer-events: none;
}
.panning,
.panning:active,
.panning .region-boundary {
  cursor: grabbing;
}
</style>
