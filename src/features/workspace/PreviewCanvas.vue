<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, shallowRef, watch } from 'vue';
import { fitZoom, initialView, locateBounds, zoomAround } from './canvasView';
import type { CanvasView } from './canvasView';
import type { UiCanvas, UiRect } from '../../shared/api/devGenerated';
import { useCompositeCanvas } from './useCompositeCanvas';
import { hitLayers } from './canvasInspection';
import type { Bounds, LayerSummary, SelectionBounds } from '../../shared/api/generated';
import {
  beginRegion,
  documentPoint,
  finishRegion,
  insideDocument,
  screenPoint,
  MIN_REGION_DRAG_CSS_PX,
} from './regionDraft';
import type { CanvasTool, Point, RegionAction, RegionDraft, RegionHandle } from './regionDraft';
import { buildSnapIndex, snapRegion } from './regionSnapping';
import type { SnapGuides, SnapIndex } from './regionSnapping';
const props = withDefaults(
  defineProps<{
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
    snapEnabled?: boolean;
    snapIndex?: SnapIndex | null;
    inspectionLayers?: readonly LayerSummary[] | null;
    autoSelect?: boolean;
    inspectionBusy?: boolean;
    selectedId?: number | null;
  }>(),
  {
    snapEnabled: true,
    snapIndex: null,
    inspectionLayers: null,
    autoSelect: false,
    inspectionBusy: false,
    selectedId: null,
  },
);
const emit = defineEmits<{
  'update:view': [view: CanvasView];
  imageError: [message: string];
  inspect: [id: number | null, contextKey: string];
  region: [bounds: SelectionBounds, contextKey: string];
  clearSelection: [contextKey: string];
}>();
const viewport = ref<HTMLElement | null>(null);
const surfaceHost = ref<HTMLDivElement | null>(null);
const dpr = ref(window.devicePixelRatio || 1);
const size = ref({ width: 0, height: 0 });
const effective = computed(() =>
  props.view.mode === 'fit'
    ? {
        ...initialView(),
        zoom: fitZoom(props.width, props.height, size.value.width, size.value.height),
      }
    : props.view,
);
const rendering = useCompositeCanvas(
  surfaceHost,
  () => ({
    url: props.url,
    width: props.width,
    height: props.height,
    viewportWidth: size.value.width,
    viewportHeight: size.value.height,
    dpr: dpr.value,
    view: effective.value,
  }),
  (message) => emit('imageError', message),
);
const ctrl = ref(false);
const candidates = shallowRef<number[]>([]);
let candidateContext = '';
const hover = shallowRef<Bounds | null>(null);
const probe = shallowRef<{
  id: number;
  x: number;
  y: number;
  moved: boolean;
  temporary: boolean;
  contextKey: string;
  candidates: number[];
} | null>(null);
const canInspect = computed(
  () =>
    !!props.url &&
    rendering.value.ready &&
    props.inspectionLayers !== null &&
    !props.disabled &&
    !props.inspectionBusy,
);
function beginInspection(event: PointerEvent) {
  if (!canInspect.value) return;
  probe.value = {
    id: event.pointerId,
    x: event.clientX,
    y: event.clientY,
    moved: false,
    temporary: event.ctrlKey,
    contextKey: props.contextKey,
    candidates: hitLayers(props.inspectionLayers!, point(event), props.width, props.height),
  };
}
function nextCandidate() {
  if (!canInspect.value || candidateContext !== props.contextKey || candidates.value.length < 2)
    return;
  const current = candidates.value.indexOf(props.selectedId ?? -1);
  emit('inspect', candidates.value[(current + 1) % candidates.value.length]!, candidateContext);
}
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
  point: Point;
  travel: number;
  alt: boolean;
  targets: SnapIndex;
  guides: SnapGuides;
} | null>(null);
const targets = computed(() => props.snapIndex ?? buildSnapIndex(props.width, props.height));
const guides = computed(() => {
  const current = regionDrag.value?.guides;
  return (['x', 'y'] as const).flatMap((axis) =>
    current?.[axis] ? [{ axis, target: current[axis].target }] : [],
  );
});
const snapNote = computed(() => {
  if (!props.snapEnabled) return '吸附已关闭';
  if (regionDrag.value?.alt) return 'Alt · 临时关闭吸附';
  return (regionDrag.value?.targets ?? targets.value).layersReady
    ? '画布与图层几何边缘 · Alt 暂停吸附'
    : '仅画布吸附 · 图层列表尚未完整';
});
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
  const id = regionDrag.value?.id ?? drag.value?.id ?? probe.value?.id;
  regionDrag.value = null;
  drag.value = null;
  probe.value = null;
  candidates.value = [];
  candidateContext = '';
  hover.value = null;
  ctrl.value = false;
  drawingNew.value = false;
  releasePointer(id);
}
function escape(event: KeyboardEvent) {
  // 长按产生的重复事件不能把一次取消草稿升级为清空已提交范围。
  if (event.repeat || event.isComposing) return;
  const wasDragging = !!regionDrag.value || !!drag.value || !!probe.value;
  cancel();
  if (!wasDragging && props.tool === 'region' && !props.disabled)
    emit('clearSelection', props.contextKey);
}
watch(
  () => [
    props.contextKey,
    props.tool,
    props.disabled,
    props.url,
    props.width,
    props.height,
    props.inspectionLayers,
    props.autoSelect,
  ],
  cancel,
  { flush: 'sync' },
);
watch(
  () => props.view,
  () => {
    if (regionDrag.value) cancel();
    hover.value = null;
    candidates.value = [];
  },
  { flush: 'sync' },
);
function refreshSnapping() {
  const region = regionDrag.value;
  if (!region) return;
  const snapped = snapRegion(
    region.draft,
    region.point,
    props.width,
    props.height,
    effective.value.zoom,
    region.targets,
    region.guides,
    props.snapEnabled && !region.alt && region.travel >= MIN_REGION_DRAG_CSS_PX,
  );
  regionDrag.value = { ...region, ...snapped };
}
function altKey(event: KeyboardEvent) {
  if (event.key === 'Control' && !event.isComposing) {
    ctrl.value = event.type === 'keydown';
    if (!ctrl.value) {
      hover.value = null;
      if (probe.value?.temporary) {
        const id = probe.value.id;
        probe.value = null;
        if (!drag.value) releasePointer(id);
      }
    }
  }
  if (event.key !== 'Alt' || event.isComposing || !regionDrag.value) return;
  regionDrag.value = { ...regionDrag.value, alt: event.type === 'keydown' };
  refreshSnapping();
}
watch(() => props.snapEnabled, refreshSnapping, { flush: 'sync' });
let dprQuery: MediaQueryList | undefined;
function observeDpr() {
  dprQuery?.removeEventListener('change', displayChanged);
  dprQuery = window.matchMedia?.(`(resolution: ${window.devicePixelRatio}dppx)`);
  dprQuery?.addEventListener('change', displayChanged);
}
function displayChanged() {
  const next = window.devicePixelRatio || 1;
  if (dpr.value !== next) {
    cancel();
    dpr.value = next;
  }
  observeDpr();
}
onMounted(() => {
  observeDpr();
  observer = new ResizeObserver((entries) => {
    const entry = entries[0];
    if (entry) {
      cancel();
      size.value = { width: entry.contentRect.width, height: entry.contentRect.height };
    }
  });
  if (viewport.value) observer.observe(viewport.value);
  window.addEventListener('blur', cancel);
  window.addEventListener('keydown', altKey);
  window.addEventListener('keyup', altKey);
  window.addEventListener('resize', displayChanged);
});
onUnmounted(() => {
  cancel();
  observer?.disconnect();
  dprQuery?.removeEventListener('change', displayChanged);
  window.removeEventListener('blur', cancel);
  window.removeEventListener('keydown', altKey);
  window.removeEventListener('keyup', altKey);
  window.removeEventListener('resize', displayChanged);
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
  if (event.button === 0 && !regionDrag.value && !drag.value) {
    startPan(event, 0);
    if (props.autoSelect || event.ctrlKey) beginInspection(event);
  }
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
  probe.value = null;
  candidates.value = [];
  hover.value = null;
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
  // Ctrl 在区域内部及控制点也临时检查，不能触发区域移动／尺寸调整。
  if (
    event.button === 0 &&
    event.ctrlKey &&
    props.tool === 'region' &&
    !drag.value &&
    !regionDrag.value &&
    !probe.value
  ) {
    event.stopPropagation();
    event.preventDefault();
    if (canInspect.value && viewport.value) {
      viewport.value.focus({ preventScroll: true });
      viewport.value.setPointerCapture(event.pointerId);
      beginInspection(event);
      ctrl.value = true;
    }
    return;
  }
  if (event.button !== 1) return;
  // 在捕获阶段处理，选框内部与控制点也能发起中键平移。
  event.stopPropagation();
  startPan(event, 1);
}
function preventMiddleDefault(event: MouseEvent) {
  if (event.button === 1) event.preventDefault();
}
function cancelPointer(event: PointerEvent) {
  if (
    event.pointerId === drag.value?.id ||
    event.pointerId === regionDrag.value?.id ||
    event.pointerId === probe.value?.id
  )
    cancel();
}
function move(event: PointerEvent) {
  const inspecting = probe.value;
  if (inspecting?.id === event.pointerId) {
    if (event.buttons & 4) {
      startPan(event, 1);
      return;
    }
    if (
      Math.hypot(event.clientX - inspecting.x, event.clientY - inspecting.y) >=
      MIN_REGION_DRAG_CSS_PX
    )
      probe.value = { ...inspecting, moved: true };
  }
  const pan = drag.value;
  if (pan?.id === event.pointerId) {
    // 鼠标多键组合只在最后一个键松开时派发 pointerup，按 buttons 检查中键提前松开。
    if (pan.button === 1 && !(event.buttons & 4)) {
      cancel();
      return;
    }
    if (
      pan.button === 0 &&
      Math.hypot(event.clientX - pan.x, event.clientY - pan.y) < MIN_REGION_DRAG_CSS_PX
    )
      return;
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
    updateDraft(event);
    return;
  }
  if (
    !probe.value &&
    canInspect.value &&
    ((props.tool === 'pan' && props.autoSelect) || event.ctrlKey)
  ) {
    const id = hitLayers(props.inspectionLayers!, point(event), props.width, props.height)[0];
    hover.value = props.inspectionLayers!.find((layer) => layer.id === id)?.bounds ?? null;
  } else hover.value = null;
}
function updateDraft(event: PointerEvent) {
  const region = regionDrag.value;
  if (!region) return;
  regionDrag.value = {
    ...region,
    point: point(event),
    travel: Math.hypot(event.clientX - region.x, event.clientY - region.y),
    alt: event.altKey,
  };
  refreshSnapping();
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
  // 框选接管有效左键手势，避免浏览器选中文字后发起原生拖放并取消指针捕获。
  event.preventDefault();
  viewport.value.focus({ preventScroll: true });
  viewport.value.setPointerCapture(event.pointerId);
  regionDrag.value = {
    id: event.pointerId,
    x: event.clientX,
    y: event.clientY,
    contextKey: props.contextKey,
    draft: beginRegion(action, start, props.region),
    point: start,
    travel: 0,
    alt: event.altKey,
    targets: targets.value,
    guides: { x: null, y: null },
  };
}
function end(event: PointerEvent) {
  const inspecting = probe.value;
  if (inspecting?.id === event.pointerId) {
    const valid =
      !inspecting.moved &&
      Math.hypot(event.clientX - inspecting.x, event.clientY - inspecting.y) <
        MIN_REGION_DRAG_CSS_PX &&
      inspecting.contextKey === props.contextKey &&
      canInspect.value &&
      (!inspecting.temporary || event.ctrlKey);
    cancel();
    ctrl.value = event.ctrlKey;
    if (valid) {
      candidates.value = inspecting.candidates;
      candidateContext = inspecting.contextKey;
      emit('inspect', inspecting.candidates[0] ?? null, inspecting.contextKey);
    }
    return;
  }
  const region = regionDrag.value;
  if (region?.id === event.pointerId) {
    updateDraft(event);
    const draft = regionDrag.value!.draft;
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
function fit() {
  setView(initialView());
}
function actualSize() {
  setView({ mode: 'manual', zoom: 1, x: 0, y: 0 });
}
/** 开发桥只读取当前交互与实际 DOM，不更改草稿、图像或视图。 */
function observation(): UiCanvas | null {
  const element = viewport.value;
  if (!element) return null;
  const pointerId = regionDrag.value?.id ?? drag.value?.id ?? probe.value?.id;
  const rect = (r: DOMRect): UiRect => ({ x: r.x, y: r.y, width: r.width, height: r.height });
  const viewportRect = element.getBoundingClientRect();
  const imagePoint = screenPoint(
    { x: 0, y: 0 },
    {
      width: props.width,
      height: props.height,
      viewportWidth: viewportRect.width,
      viewportHeight: viewportRect.height,
      view: effective.value,
    },
  );
  return {
    viewport: rect(viewportRect),
    image: rendering.value.decoded
      ? {
          x: viewportRect.x + imagePoint.x,
          y: viewportRect.y + imagePoint.y,
          width: props.width * effective.value.zoom,
          height: props.height * effective.value.zoom,
        }
      : null,
    documentWidth: props.width,
    documentHeight: props.height,
    imageDecoded: rendering.value.decoded,
    paintPrepared: rendering.value.ready,
    imageError: rendering.value.error,
    surface: rendering.value.surface,
    sceneRevision: rendering.value.generation.toString(),
    paintedRevision: rendering.value.paintedGeneration.toString(),
    inspection: {
      ready: props.inspectionLayers !== null,
      selectedLayerId: props.selectedId,
      detailsLayerId: null,
      treeSelectedLayerId: null,
      filterActive: false,
      autoSelect: props.autoSelect,
      ctrl: ctrl.value,
      pending: props.inspectionBusy,
      candidateCount: candidates.value.length,
      precision: 'bounds',
    },
    zoom: effective.value.zoom,
    panX: effective.value.x,
    panY: effective.value.y,
    panning: !!drag.value,
    pointerCaptured: pointerId !== undefined && element.hasPointerCapture(pointerId),
    draft: regionDrag.value ? { ...regionDrag.value.draft.bounds } : null,
    alt: regionDrag.value?.alt ?? false,
    snapGuides: guides.value.length,
    snapX: regionDrag.value?.guides.x
      ? {
          position: regionDrag.value.guides.x.target.value,
          layerId: regionDrag.value.guides.x.target.layerId,
        }
      : null,
    snapY: regionDrag.value?.guides.y
      ? {
          position: regionDrag.value.guides.y.target.value,
          layerId: regionDrag.value.guides.y.target.layerId,
        }
      : null,
  };
}
defineExpose({ locate, cancel, fit, actualSize, observation });
</script>
<template>
  <div class="preview-pane">
    <div class="canvas-controls" aria-label="选区操作">
      <span class="preview-label">合成预览 · 几何边界检查</span>
      <button
        v-if="candidates.length > 1"
        data-dev-target="canvas:next"
        :disabled="!canInspect"
        @click="nextCandidate"
      >
        切换重叠图层（{{ candidates.length }}）
      </button>
      <span v-if="ctrl" class="preview-label">Ctrl · 临时检查</span>
      <div class="region-controls">
        <span v-if="tool === 'region'" class="snap-note" :title="snapNote">{{ snapNote }}</span>
        <span v-if="tool === 'region'">{{
          drawingNew ? '在原选框内也可开始新范围' : '松开确认 · 拖动时 Esc 取消，空闲时清空'
        }}</span>
        <button
          v-if="tool === 'region' && region"
          data-dev-target="canvas:reframe"
          :disabled="disabled"
          :aria-pressed="drawingNew"
          @click="
            cancel();
            drawingNew = true;
          "
        >
          重新框选
        </button>
      </div>
    </div>
    <div
      ref="viewport"
      data-dev-target="canvas"
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
      @pointerleave="hover = null"
      @pointerup="end"
      @pointercancel="cancelPointer"
      @lostpointercapture="cancelPointer"
      @keydown.esc.prevent="escape"
      @keydown.left.prevent="setView({ ...effective, mode: 'manual', x: effective.x - 30 })"
      @keydown.right.prevent="setView({ ...effective, mode: 'manual', x: effective.x + 30 })"
      @keydown.up.prevent="setView({ ...effective, mode: 'manual', y: effective.y - 30 })"
      @keydown.down.prevent="setView({ ...effective, mode: 'manual', y: effective.y + 30 })"
    >
      <div
        v-if="url && rendering.decoded"
        class="composite-background"
        :style="boundaryStyle({ x: 0, y: 0, width, height })"
      ></div>
      <div
        ref="surfaceHost"
        class="composite-surface"
        role="img"
        :aria-label="`${name} 保存时合成预览`"
      ></div>
      <div v-if="hover" class="hover-boundary" :style="boundaryStyle(hover)"></div>
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
            :data-dev-target="`handle:${handle.id}`"
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
      <div
        v-for="guide in guides"
        :key="guide.axis"
        class="snap-guide"
        :class="`snap-guide-${guide.axis}`"
        :data-snap-axis="guide.axis"
        :aria-label="`吸附目标：${guide.target.label}`"
        :style="
          boundaryStyle(
            guide.axis === 'x'
              ? { x: guide.target.value, y: 0, width: 0, height }
              : { x: 0, y: guide.target.value, width, height: 0 },
          )
        "
      ></div>
      <div v-if="guides.length" class="snap-feedback">
        {{ guides.map((guide) => guide.target.label).join(' / ') }}
      </div>
      <div v-if="!url" class="canvas-message"><slot /></div>
    </div>
    <div class="canvas-footer">
      <div class="zoom-controls" aria-label="画布视图">
        <button aria-label="缩小" @click="zoom(0.8)">−</button>
        <output aria-label="当前缩放">{{ Number((effective.zoom * 100).toFixed(2)) }}%</output>
        <button aria-label="放大" @click="zoom(1.25)">+</button>
        <button title="实际像素（Ctrl+1）" @click="actualSize">100%</button>
        <button title="适应窗口（Ctrl+0）" @click="fit">适应窗口</button>
      </div>
      <span class="canvas-dimensions">{{ width }} × {{ height }} px</span>
    </div>
  </div>
</template>

<style scoped>
.composite-surface {
  position: absolute;
  inset: 0;
  pointer-events: none;
}
.composite-background {
  position: absolute;
  pointer-events: none;
  background: repeating-conic-gradient(#ddd 0% 25%, #fff 0% 50%) 0 / 16px 16px;
  box-shadow: 0 2px 12px #0008;
}
.hover-boundary {
  position: absolute;
  border: 1px dashed #91b9e8;
  pointer-events: none;
  box-sizing: border-box;
}
.snap-guide {
  position: absolute;
  z-index: 3;
  pointer-events: none;
}
.snap-guide-x {
  border-left: 1px dashed #e39ad8;
}
.snap-guide-y {
  border-top: 1px dashed #e39ad8;
}
.snap-feedback {
  position: absolute;
  z-index: 3;
  top: 8px;
  left: 8px;
  max-width: calc(100% - 32px);
  padding: 3px 6px;
  background: #29212fee;
  color: #f0bce9;
  font-size: 11px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  pointer-events: none;
}
.region-tool,
.region-tool:active {
  cursor: crosshair;
}
.region-boundary {
  position: absolute;
  z-index: 2;
  box-sizing: border-box;
  border: 1px solid var(--accent);
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
  border: 1px solid var(--accent);
  transform: translate(-50%, -50%);
}
.region-size {
  position: absolute;
  left: 0;
  bottom: calc(100% + 7px);
  padding: 2px 5px;
  background: #264b71;
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
