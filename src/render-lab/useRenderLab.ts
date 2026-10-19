import { computed, onMounted, onScopeDispose, reactive, ref, shallowRef, watch } from 'vue';
import { subscribeWorkspace, workspaceAction, workspaceMessage } from '../shared/api/workspace';
import type { WorkspaceSnapshot } from '../shared/api/generated';
import type { UiRenderLab } from '../shared/api/devGenerated';
import { useDevObservation } from '../features/workspace/useDevObservation';
import { readScene, readResource, largeExperiment } from './scene';
import type { RenderScene, RenderProfile } from './generated';
import { prepareAssets, hitCandidates, type Assets } from './resources';
import { createRenderer, nextPaint, type Renderer, type View, type TextMode } from './renderer';

/** 独立实验视图状态；文档和检查选择始终从核心快照取得。 */
export function useRenderLab() {
  const host = ref<HTMLElement>(),
    viewport = ref<HTMLElement>();
  const snapshot = shallowRef<WorkspaceSnapshot | null>(null),
    scene = shallowRef<RenderScene | null>(null),
    assets = shallowRef<Assets | null>(null);
  const active = computed(
    () => snapshot.value?.documents.find((d) => d.id === snapshot.value?.activeDocumentId) ?? null,
  );
  const profile = ref<RenderProfile>(largeExperiment ? 'large' : 'bounded');
  let loading: AbortController | null = null;
  const view = reactive<View>({
    width: 640,
    height: 420,
    dpr: window.devicePixelRatio,
    zoom: 1,
    panX: 16,
    panY: 16,
    textMode: 'shared',
  });
  const status = reactive<UiRenderLab>({
    surface: null,
    engine: 'konva',
    phase: 'idle',
    generation: '0',
    sourceSha256: null,
    fontSha256: null,
    fontReady: false,
    nodeCount: 0,
    resourceCount: 0,
    selectedLayerId: null,
    candidateIds: [],
    hoverLayerId: null,
    autoSelect: true,
    ctrl: false,
    initMs: 0,
    renderMs: 0,
    sceneReadMs: 0,
    assetsMs: 0,
    firstReadyMs: 0,
    textMode: 'shared',
    nativeTextLayers: 0,
    sharedTextLayers: 0,
    unsupportedLayers: 0,
    profile: profile.value,
    sourceLayers: 0,
    drawnLayerIds: [],
    rasterLayerIds: [],
    degradedLayerIds: [],
    unsupportedLayerIds: [],
    textRasterLayerIds: [],
    encodedBytes: 0,
    alphaBytes: 0,
    selectionMs: null,
    hitMs: null,
    samples: [],
    backend: '',
    error: null,
    limitations: [],
  });
  const selecting = ref(false),
    panning = ref(false),
    pointerCaptured = ref(false);
  let renderer: Renderer | null = null,
    serial = 0,
    loadSerial = 0,
    disposed = false,
    unsubscribe: (() => void) | undefined;
  let pan: { id: number; x: number; y: number; panX: number; panY: number } | null = null;
  let drawingPan = false,
    pendingPan = false;
  let inputAt: number | null = null,
    previousPanFrame: number | null = null;
  const readbackCanvas = document.createElement('canvas');
  readbackCanvas.width = readbackCanvas.height = 1;
  const readback = readbackCanvas.getContext('2d', { willReadFrequently: true });
  const apply = (value: WorkspaceSnapshot) => {
    if (!snapshot.value || BigInt(value.sequence) >= BigInt(snapshot.value.sequence))
      snapshot.value = value;
  };
  const fail = (error: unknown) => {
    status.error = workspaceMessage(error).slice(0, 2048);
    status.phase = 'error';
  };
  function releaseRenderer() {
    renderer?.dispose();
    renderer = null;
    host.value?.replaceChildren();
    status.nodeCount = 0;
    status.resourceCount = 0;
    status.backend = '';
  }
  function release() {
    loading?.abort();
    loading = null;
    pendingPan = false;
    endPan();
    serial++;
    loadSerial++;
    releaseRenderer();
    assets.value?.dispose();
    assets.value = null;
    scene.value = null;
    status.fontReady = false;
    status.fontSha256 = null;
    status.sourceSha256 = null;
    status.candidateIds = [];
    status.hoverLayerId = null;
    status.phase = 'released';
    status.samples = [];
    status.nativeTextLayers = status.sharedTextLayers = status.unsupportedLayers = 0;
    status.sourceLayers = status.encodedBytes = status.alphaBytes = 0;
    status.drawnLayerIds = [];
    status.rasterLayerIds = [];
    status.degradedLayerIds = [];
    status.unsupportedLayerIds = [];
    status.textRasterLayerIds = [];
    status.selectionMs = status.hitMs = null;
    status.sceneReadMs =
      status.assetsMs =
      status.firstReadyMs =
      status.initMs =
      status.renderMs =
        0;
    previousPanFrame = inputAt = null;
    status.generation = String(serial);
  }
  async function render(rebuild = false, operation = rebuild ? 'rebuild' : 'zoom') {
    const resource = assets.value,
      currentScene = scene.value,
      container = host.value;
    if (!resource || !currentScene || !container || disposed) return;
    const token = ++serial,
      transform = { ...view };
    status.generation = String(token);
    status.phase = 'loading';
    status.error = null;
    status.hoverLayerId = null;
    let candidate = renderer;
    try {
      if (rebuild || !candidate) {
        releaseRenderer();
        const mount = document.createElement('div');
        container.replaceChildren(mount);
        const start = performance.now();
        candidate = await createRenderer(mount, resource, transform);
        if (disposed || token !== serial) {
          candidate.dispose();
          return;
        }
        renderer = candidate;
        status.initMs = performance.now() - start;
      }
      const start = performance.now();
      const input = inputAt;
      const frameIntervalMs =
        operation === 'pan' && previousPanFrame !== null ? start - previousPanFrame : null;
      if (operation === 'pan') previousPanFrame = start;
      const submitMs = await candidate.draw(transform);
      const syncStart = performance.now();
      const synchronized = operation !== 'pan';
      if (synchronized) {
        if (!readback) throw new Error('无法创建统一 Canvas 读回屏障');
        // 同一屏障强制完成离屏像素读取；不代表物理显示。连续手势不引入读回开销。
        readback.clearRect(0, 0, 1, 1);
        readback.drawImage(candidate.canvas(), 0, 0, 1, 1);
        readback.getImageData(0, 0, 1, 1);
      }
      const readbackMs = synchronized ? performance.now() - syncStart : 0;
      const duration = performance.now() - start;
      const inputToReadyMs = input === null ? null : performance.now() - input;
      if (synchronized) await nextPaint();
      if (disposed || token !== serial) return;
      status.samples.push({
        generation: String(token),
        operation,
        submitMs,
        readyMs: duration,
        readbackMs,
        synchronized,
        frameIntervalMs,
        inputToReadyMs,
      });
      if (status.samples.length > 60) status.samples.splice(0, status.samples.length - 60);
      status.renderMs = duration;
      status.backend = candidate.backend;
      status.nodeCount = candidate.nodeCount;
      status.resourceCount = candidate.resourceCount;
      status.textMode = transform.textMode;
      status.nativeTextLayers = candidate.nativeTextLayers;
      status.sharedTextLayers = candidate.sharedTextLayers;
      status.unsupportedLayers = resource.layers.filter((l) => l.layer.visible && l.problem).length;
      status.profile = profile.value;
      status.sourceLayers = currentScene.layers.length;
      const visible = resource.layers.filter((l) => l.layer.visible && l.layer.opacity > 0);
      const drawn = visible.filter((l) => !l.problem && l.layer.content.kind !== 'group');
      status.drawnLayerIds = drawn.map((l) => l.layer.id);
      status.rasterLayerIds = drawn
        .filter((l) => l.layer.content.kind === 'bitmap')
        .map((l) => l.layer.id);
      status.textRasterLayerIds = drawn.filter((l) => l.textRasterReason).map((l) => l.layer.id);
      status.degradedLayerIds = drawn
        .filter((l) => l.layer.compositionWarning !== null)
        .map((l) => l.layer.id);
      status.unsupportedLayerIds = visible.filter((l) => l.problem).map((l) => l.layer.id);
      status.encodedBytes = resource.encodedBytes;
      status.alphaBytes = resource.alphaBytes;
      status.phase = 'ready';
    } catch (error) {
      if (!disposed && token === serial) {
        releaseRenderer();
        fail(error);
      }
    }
  }
  async function load() {
    release();
    const document = active.value;
    if (!document) return;
    const token = ++loadSerial;
    const abort = new AbortController();
    loading = abort;
    status.phase = 'loading';
    status.error = null;
    try {
      const started = performance.now();
      const currentProfile = profile.value;
      const input = await readScene(document.id, document.revision, currentProfile);
      if (disposed || token !== loadSerial) return;
      status.sceneReadMs = performance.now() - started;
      const resourcesAt = performance.now();
      const resource = await prepareAssets(
        input,
        largeExperiment
          ? (id) => readResource(document.id, document.revision, currentProfile, id)
          : undefined,
        abort.signal,
      );
      if (disposed || token !== loadSerial) {
        resource.dispose();
        return;
      }
      scene.value = input;
      status.assetsMs = performance.now() - resourcesAt;
      assets.value = resource;
      status.sourceSha256 = input.sourceSha256;
      status.fontSha256 = resource.fontHash;
      status.fontReady = true;
      status.limitations = resource.limitations;
      if (currentProfile === 'largeLayers')
        status.limitations.unshift(
          '源图层诊断投影：不执行完整画板、组、蒙版、效果、剪贴和 ICC 合成；不能作为整页保真通过。',
        );
      view.zoom = 1;
      view.panX = 16;
      view.panY = 16;
      if (largeExperiment) fitTransform();
      await render(true);
      if (!disposed && token === loadSerial && String(status.phase) === 'ready')
        status.firstReadyMs = performance.now() - started;
    } catch (error) {
      if (!disposed && token === loadSerial) fail(error);
    }
  }
  async function rebuild() {
    if (status.phase === 'loading') return;
    if (!active.value) return;
    if (!assets.value) await load();
    else await render(true);
  }
  async function zoom(value: number) {
    if (status.phase !== 'ready') return;
    view.zoom = Math.max(largeExperiment ? 0.01 : 0.25, Math.min(4, value));
    inputAt = performance.now();
    await render();
  }
  async function textMode(mode: TextMode) {
    if (status.phase === 'loading') return;
    view.textMode = mode;
    status.textMode = mode;
    if (assets.value) await render(true);
  }
  function clearMetrics() {
    status.samples = [];
    status.selectionMs = status.hitMs = null;
    previousPanFrame = inputAt = null;
  }
  async function viewportSize(large: boolean) {
    if (status.phase === 'loading') return;
    view.width = large ? 960 : 640;
    view.height = large ? 600 : 420;
    if (assets.value) await render(true, 'resize');
  }
  function fitTransform() {
    if (!scene.value) return;
    view.zoom = Math.min(
      (view.width - 32) / scene.value.width,
      (view.height - 32) / scene.value.height,
      4,
    );
    view.panX = (view.width - scene.value.width * view.zoom) / 2;
    view.panY = (view.height - scene.value.height * view.zoom) / 2;
  }
  async function fit() {
    if (status.phase !== 'ready') return;
    fitTransform();
    await render(false, 'fit');
  }
  async function focusLayer() {
    if (status.phase !== 'ready') return;
    const layer = scene.value?.layers.find((l) => l.id === active.value?.selectedLayerId);
    if (!layer) return;
    view.panX = view.width / 2 - (layer.bounds.x + layer.bounds.width / 2) * view.zoom;
    view.panY = view.height / 2 - (layer.bounds.y + layer.bounds.height / 2) * view.zoom;
    await render(false, 'focus');
  }
  async function sourceLayers(enabled: boolean) {
    if (!largeExperiment || status.phase === 'loading') return;
    profile.value = enabled ? 'largeLayers' : 'large';
    status.profile = profile.value;
    if (active.value) await load();
  }
  async function reset() {
    if (status.phase !== 'ready') return;
    view.panX = 16;
    view.panY = 16;
    view.zoom = 1;
    await render(false, 'reset');
  }
  async function select(id: number | null) {
    const document = active.value;
    if (!document || selecting.value) return;
    const requestedGeneration = loadSerial;
    const started = performance.now();
    selecting.value = true;
    try {
      apply(
        await workspaceAction({
          kind: 'selectLayer',
          documentId: document.id,
          revision: document.revision,
          layerId: id,
        }),
      );
      if (!disposed && requestedGeneration === loadSerial)
        status.selectionMs = performance.now() - started;
    } catch (error) {
      if (
        !disposed &&
        requestedGeneration === loadSerial &&
        active.value?.id === document.id &&
        active.value.revision === document.revision
      )
        fail(error);
    } finally {
      selecting.value = false;
    }
  }
  function candidates(event: PointerEvent) {
    const bounds = viewport.value?.getBoundingClientRect();
    const started = performance.now();
    const hits =
      bounds && assets.value
        ? hitCandidates(
            assets.value,
            (event.clientX - bounds.x - view.panX) / view.zoom,
            (event.clientY - bounds.y - view.panY) / view.zoom,
          )
        : [];
    status.hitMs = performance.now() - started;
    return hits.slice(0, 64);
  }
  function pointerDown(event: PointerEvent) {
    if (status.phase !== 'ready') return;
    viewport.value?.focus();
    if (event.button === 1) {
      event.preventDefault();
      previousPanFrame = null;
      pan = {
        id: event.pointerId,
        x: event.clientX,
        y: event.clientY,
        panX: view.panX,
        panY: view.panY,
      };
      viewport.value?.setPointerCapture(event.pointerId);
      panning.value = true;
      pointerCaptured.value = true;
    } else if (event.button === 0 && (status.autoSelect || event.ctrlKey)) {
      const hits = candidates(event);
      status.candidateIds = hits.map((h) => h.id);
      void select(hits[0]?.id ?? null);
    }
  }
  function pointerMove(event: PointerEvent) {
    if (pan) {
      inputAt = performance.now();
      view.panX = pan.panX + event.clientX - pan.x;
      view.panY = pan.panY + event.clientY - pan.y;
      void drawPan();
    } else if (status.phase === 'ready')
      status.hoverLayerId =
        status.autoSelect || event.ctrlKey ? (candidates(event)[0]?.id ?? null) : null;
  }
  async function drawPan() {
    pendingPan = true;
    if (drawingPan) return;
    drawingPan = true;
    try {
      while (pendingPan && !disposed && assets.value) {
        pendingPan = false;
        await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
        if (!disposed && assets.value) await render(false, 'pan');
      }
    } finally {
      drawingPan = false;
    }
  }
  function endPan() {
    const id = pan?.id;
    pan = null;
    panning.value = false;
    pointerCaptured.value = false;
    if (id !== undefined && viewport.value?.hasPointerCapture(id))
      viewport.value.releasePointerCapture(id);
  }
  function blur() {
    status.ctrl = false;
    status.hoverLayerId = null;
    endPan();
  }
  function key(event: KeyboardEvent) {
    if (event.key === 'Control') status.ctrl = event.type === 'keydown';
    if (!status.ctrl && !status.autoSelect) status.hoverLayerId = null;
  }
  function wheel(event: WheelEvent) {
    event.preventDefault();
    if (status.phase === 'ready') void zoom(view.zoom * (event.deltaY < 0 ? 1.25 : 0.8));
  }
  function next() {
    const ids = status.candidateIds;
    if (!ids.length) return;
    const index = ids.indexOf(active.value?.selectedLayerId ?? -1);
    void select(ids[(index + 1) % ids.length] ?? null);
  }
  const selected = computed(
    () => scene.value?.layers.find((l) => l.id === active.value?.selectedLayerId) ?? null,
  );
  const outline = computed(() => {
    const b = selected.value?.bounds;
    return b
      ? {
          left: view.panX + b.x * view.zoom + 'px',
          top: view.panY + b.y * view.zoom + 'px',
          width: b.width * view.zoom + 'px',
          height: b.height * view.zoom + 'px',
        }
      : null;
  });
  const hoverOutline = computed(() => {
    const b = scene.value?.layers.find((l) => l.id === status.hoverLayerId)?.bounds;
    return b
      ? {
          left: view.panX + b.x * view.zoom + 'px',
          top: view.panY + b.y * view.zoom + 'px',
          width: b.width * view.zoom + 'px',
          height: b.height * view.zoom + 'px',
        }
      : null;
  });
  useDevObservation(() => {
    const b = viewport.value?.getBoundingClientRect();
    const region = b ? { x: b.x, y: b.y, width: b.width, height: b.height } : null;
    const canvas = renderer?.canvas();
    const canvasBounds = canvas?.getBoundingClientRect();
    return {
      coreSequence: snapshot.value?.sequence ?? null,
      sessionId: snapshot.value?.selection.sessionId ?? null,
      activeDocument: active.value
        ? { id: active.value.id, revision: active.value.revision }
        : null,
      selectionRevision: snapshot.value?.selection.selectionRevision ?? null,
      snapshotId: null,
      region: null,
      tool: 'pan',
      panel: 'layers',
      tasks: [],
      selectionRunning: selecting.value,
      content: null,
      cleanup: {
        opened: false,
        phase: null,
        planId: null,
        taskIds: [],
        syncing: false,
        syncError: false,
        pending: false,
      },
      canvas:
        region && scene.value
          ? {
              viewport: region,
              image: {
                x: region.x + view.panX,
                y: region.y + view.panY,
                width: scene.value.width * view.zoom,
                height: scene.value.height * view.zoom,
              },
              documentWidth: scene.value.width,
              documentHeight: scene.value.height,
              imageDecoded: status.fontReady,
              paintPrepared: status.phase === 'ready',
              imageError: status.error,
              zoom: view.zoom,
              panX: view.panX,
              panY: view.panY,
              panning: panning.value,
              pointerCaptured: pointerCaptured.value,
              draft: null,
              alt: false,
              snapGuides: 0,
              snapX: null,
              snapY: null,
            }
          : null,
      renderLab: {
        ...status,
        surface:
          canvas && canvasBounds
            ? {
                width: canvas.width,
                height: canvas.height,
                cssWidth: canvasBounds.width,
                cssHeight: canvasBounds.height,
                configuredDpr: view.dpr,
              }
            : null,
        candidateIds: [...status.candidateIds],
        limitations: [...status.limitations],
        samples: status.samples.map((s) => ({ ...s })),
        drawnLayerIds: [...status.drawnLayerIds],
        rasterLayerIds: [...status.rasterLayerIds],
        degradedLayerIds: [...status.degradedLayerIds],
        unsupportedLayerIds: [...status.unsupportedLayerIds],
        textRasterLayerIds: [...status.textRasterLayerIds],
        selectedLayerId: active.value?.selectedLayerId ?? null,
      },
    };
  });
  watch(
    () => active.value?.id + ':' + active.value?.revision,
    () => void load(),
  );
  watch(
    () => status.autoSelect,
    (enabled) => {
      if (!enabled && !status.ctrl) status.hoverLayerId = null;
    },
  );
  onMounted(async () => {
    window.addEventListener('blur', blur);
    window.addEventListener('keydown', key);
    window.addEventListener('keyup', key);
    try {
      const stop = await subscribeWorkspace(apply, fail);
      if (disposed) {
        stop();
        return;
      }
      unsubscribe = stop;
      apply(await workspaceAction({ kind: 'snapshot' }));
    } catch (error) {
      if (!disposed) fail(error);
    }
  });
  onScopeDispose(() => {
    disposed = true;
    release();
    unsubscribe?.();
    window.removeEventListener('blur', blur);
    window.removeEventListener('keydown', key);
    window.removeEventListener('keyup', key);
  });
  const setHost = (element: unknown) => {
    host.value = element instanceof HTMLElement ? element : undefined;
  };
  const setViewport = (element: unknown) => {
    viewport.value = element instanceof HTMLElement ? element : undefined;
  };
  return {
    setHost,
    setViewport,
    active,
    scene,
    assets,
    status,
    view,
    textMode,
    clearMetrics,
    viewportSize,
    largeExperiment,
    sourceLayers,
    fit,
    focusLayer,
    outline,
    hoverOutline,
    rebuild,
    zoom,
    reset,
    release,
    pointerDown,
    pointerMove,
    endPan,
    wheel,
    next,
    select,
  };
}
