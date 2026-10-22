import Konva from 'konva';
import type { CanvasView } from './canvasView';
import { ownPreviewResource } from './previewResources';

/** 一个活动文档只持有一张合成图；URL 的所有权仍属于 usePreview。 */
export interface CompositeFrame {
  url: string | null;
  width: number;
  height: number;
  viewportWidth: number;
  viewportHeight: number;
  dpr: number;
  view: CanvasView;
}
export interface CompositeState {
  generation: number;
  paintedGeneration: number;
  decoded: boolean;
  ready: boolean;
  error: string | null;
  surface: {
    width: number;
    height: number;
    cssWidth: number;
    cssHeight: number;
    configuredDpr: number;
  } | null;
}
export const emptyCompositeState = (): CompositeState => ({
  generation: 0,
  paintedGeneration: 0,
  decoded: false,
  ready: false,
  error: null,
  surface: null,
});
// 限制的是视口 backing store，和 PSD 源／解码预算分开。不得分配文档大小的 Canvas。
const MAX_SURFACE_PIXELS = 16_777_216;
const MAX_SURFACE_EDGE = 8192;
export function validSurface(width: number, height: number, dpr: number): boolean {
  return (
    [width, height, dpr].every(Number.isFinite) &&
    width > 0 &&
    height > 0 &&
    dpr > 0 &&
    dpr <= 8 &&
    Math.ceil(width * dpr) <= MAX_SURFACE_EDGE &&
    Math.ceil(height * dpr) <= MAX_SURFACE_EDGE &&
    Math.ceil(width * dpr) * Math.ceil(height * dpr) <= MAX_SURFACE_PIXELS
  );
}

/** 绘制、图片解码、RAF 和 Konva 节点均由此实例持有，销毁后迟到回调不能发布。 */
export function createCompositeRenderer(
  host: HTMLDivElement,
  publish: (value: CompositeState) => void,
) {
  let stage: Konva.Stage | null = null;
  let layer: Konva.Layer | null = null;
  let node: Konva.Image | null = null;
  let image: HTMLImageElement | null = null;
  let source = '';
  let frame: CompositeFrame | null = null;
  const state = emptyCompositeState();
  let disposed = false;
  const finishRenderer = ownPreviewResource('renderers');
  let finishStage: (() => void) | null = null;
  let finishImage: (() => void) | null = null;
  type PendingFrame = { id: number; finish: () => void };
  let drawFrame: PendingFrame | null = null;
  let readyFrame: PendingFrame | null = null;
  function requestFrame(callback: () => void): PendingFrame {
    const finish = ownPreviewResource('frames');
    try {
      return {
        id: requestAnimationFrame(() => {
          finish();
          callback();
        }),
        finish,
      };
    } catch (cause) {
      finish();
      throw cause;
    }
  }
  const report = () => {
    if (!disposed) publish({ ...state });
  };
  function cancelFrames() {
    for (const frame of [drawFrame, readyFrame]) {
      if (frame) {
        cancelAnimationFrame(frame.id);
        frame.finish();
      }
    }
    drawFrame = readyFrame = null;
  }
  function releaseImage() {
    node?.image(undefined);
    if (image) {
      image.onload = image.onerror = null;
      image.removeAttribute('src');
    }
    image = null;
    finishImage?.();
    finishImage = null;
    state.decoded = false;
  }
  function releaseSurface() {
    node?.image(undefined);
    stage?.destroy();
    finishStage?.();
    finishStage = null;
    stage = layer = node = null;
    state.surface = null;
  }
  function fail(cause: unknown) {
    cancelFrames();
    releaseImage();
    releaseSurface();
    state.ready = false;
    state.error = cause instanceof Error ? cause.message : String(cause);
    report();
  }
  function draw() {
    drawFrame = null;
    const f = frame;
    if (disposed || !f || !f.url || !image || !state.decoded || state.error) return;
    if (f.viewportWidth <= 0 || f.viewportHeight <= 0) return;
    if (!validSurface(f.viewportWidth, f.viewportHeight, f.dpr)) {
      fail(new Error('画布视口超出显示预算，请缩小窗口后重试预览。'));
      return;
    }
    try {
      if (!stage) {
        // 从零尺寸建立，校验之后才分配。禁用命中图；检查选择使用 Rust 图层摘要。
        stage = new Konva.Stage({ container: host, width: 0, height: 0, listening: false });
        finishStage = ownPreviewResource('stages');
        layer = new Konva.Layer({ listening: false, imageSmoothingEnabled: true });
        node = new Konva.Image({ image, listening: false, perfectDrawEnabled: false });
        layer.add(node);
        stage.add(layer);
      }
      const canvas = layer!.getCanvas();
      // DPR 上升而 CSS 视口缩小时，先归零；避免按“旧大视口 × 新 DPR”临时超额分配。
      if (
        canvas.getPixelRatio() !== f.dpr ||
        stage.width() !== f.viewportWidth ||
        stage.height() !== f.viewportHeight
      )
        stage.size({ width: 0, height: 0 });
      if (canvas.getPixelRatio() !== f.dpr) canvas.setPixelRatio(f.dpr);
      stage.size({ width: f.viewportWidth, height: f.viewportHeight });
      node!.setAttrs({
        image,
        width: f.width,
        height: f.height,
        x: f.viewportWidth / 2 + f.view.x - (f.width * f.view.zoom) / 2,
        y: f.viewportHeight / 2 + f.view.y - (f.height * f.view.zoom) / 2,
        scaleX: f.view.zoom,
        scaleY: f.view.zoom,
      });
      layer!.draw();
      const element = layer!.getNativeCanvasElement();
      const rect = element.getBoundingClientRect();
      state.surface = {
        width: element.width,
        height: element.height,
        cssWidth: rect.width,
        cssHeight: rect.height,
        configuredDpr: canvas.getPixelRatio(),
      };
      const generation = state.generation;
      // 下一帧才暴露 ready；缩放／换图／DPR 变化撤销旧代次的确认。
      readyFrame = requestFrame(() => {
        readyFrame = null;
        if (!disposed && generation === state.generation) {
          state.paintedGeneration = generation;
          state.ready = true;
          report();
        }
      });
      report();
    } catch (cause) {
      fail(cause);
    }
  }
  function schedule() {
    cancelFrames();
    drawFrame = requestFrame(draw);
  }
  function update(next: CompositeFrame) {
    if (disposed) return;
    frame = next;
    state.generation++;
    state.ready = false;
    cancelFrames();
    const nextSource = `${next.url ?? ''}/${next.width}/${next.height}`;
    if (nextSource !== source) {
      source = nextSource;
      releaseImage();
      releaseSurface();
      state.error = null;
      if (next.url) {
        const loading = new Image();
        image = loading;
        finishImage = ownPreviewResource('images');
        loading.onload = () => {
          loading.onload = null;
          const finishDecode = ownPreviewResource('decodes');
          void (async () => {
            try {
              await loading.decode();
              if (disposed || image !== loading) return;
              if (loading.naturalWidth !== next.width || loading.naturalHeight !== next.height)
                throw new Error('合成预览尺寸与当前文档不一致。');
              state.decoded = true;
              loading.onload = loading.onerror = null;
              report();
              schedule();
            } catch (cause: unknown) {
              if (!disposed && image === loading) fail(cause);
            } finally {
              // 销毁后在途 decode 仍计费至实际 settle，不能提前冒充释放。
              finishDecode();
            }
          })();
        };
        loading.onerror = () => {
          if (image === loading) fail(new Error('浏览器无法解码合成预览。'));
        };
        loading.src = next.url;
      }
    }
    if (state.decoded) schedule();
    report();
  }
  function dispose() {
    if (disposed) return;
    disposed = true;
    cancelFrames();
    releaseImage();
    releaseSurface();
    frame = null;
    finishRenderer();
  }
  return { update, dispose };
}
