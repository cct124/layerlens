import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { createCompositeRenderer, validSurface } from './compositeRenderer';
import type { CompositeFrame, CompositeState } from './compositeRenderer';
import { readPreviewResources } from './previewResources';

// 绘图正确性由 MCP 的真实截图验收；此处隔离浏览器资源，固定解码／RAF 交错。
const resource = vi.hoisted(() => ({ created: 0, destroyed: 0, draws: 0, maxWidth: 0 }));
vi.mock('konva', () => {
  class Layer {
    element = document.createElement('canvas');
    ratio = 1;
    add() {}
    draw() {
      resource.draws++;
    }
    getCanvas() {
      return {
        getPixelRatio: () => this.ratio,
        setPixelRatio: (n: number) => {
          this.element.width = (this.element.width / this.ratio) * n;
          this.element.height = (this.element.height / this.ratio) * n;
          this.ratio = n;
          resource.maxWidth = Math.max(resource.maxWidth, this.element.width);
        },
      };
    }
    getNativeCanvasElement() {
      return this.element;
    }
  }
  return {
    default: {
      Stage: class {
        layer: Layer | null = null;
        w = 0;
        h = 0;
        width() {
          return this.w;
        }
        height() {
          return this.h;
        }
        constructor() {
          resource.created++;
        }
        add(layer: Layer) {
          this.layer = layer;
        }
        size({ width, height }: { width: number; height: number }) {
          this.w = width;
          this.h = height;
          if (this.layer) {
            this.layer.element.width = Math.floor(width * this.layer.ratio);
            this.layer.element.height = Math.floor(height * this.layer.ratio);
            this.layer.element.getBoundingClientRect = () => new DOMRect(0, 0, width, height);
          }
        }
        destroy() {
          resource.destroyed++;
          this.layer = null;
        }
      },
      Layer,
      Image: class {
        image() {}
        setAttrs() {}
      },
    },
  };
});
class DecodingImage {
  src = '';
  naturalWidth = 100;
  naturalHeight = 80;
  onload: (() => void) | null = null;
  onerror: (() => void) | null = null;
  finish!: () => void;
  decoded = new Promise<void>((resolve) => {
    this.finish = resolve;
  });
  decode() {
    return this.decoded;
  }
  removeAttribute() {
    this.src = '';
  }
}
let images: DecodingImage[], frames: Map<number, FrameRequestCallback>, nextFrame: number;
let renderer: ReturnType<typeof createCompositeRenderer>;
let reports: CompositeState[];
const frame: CompositeFrame = {
  url: 'blob:a',
  width: 100,
  height: 80,
  viewportWidth: 400,
  viewportHeight: 300,
  dpr: 1.5,
  view: { mode: 'manual', zoom: 1, x: 0, y: 0 },
};
beforeEach(() => {
  images = [];
  frames = new Map();
  nextFrame = 0;
  reports = [];
  resource.created = resource.destroyed = resource.draws = 0;
  resource.maxWidth = 0;
  vi.stubGlobal(
    'Image',
    class extends DecodingImage {
      constructor() {
        super();
        images.push(this);
      }
    },
  );
  vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => {
    frames.set(++nextFrame, callback);
    return nextFrame;
  });
  vi.stubGlobal('cancelAnimationFrame', (id: number) => frames.delete(id));
  renderer = createCompositeRenderer(document.createElement('div'), (s) => reports.push(s));
});
afterEach(() => {
  renderer.dispose();
  vi.unstubAllGlobals();
});
function tick() {
  const callbacks = [...frames.values()];
  frames.clear();
  callbacks.forEach((callback) => callback(0));
}
async function decode(image: DecodingImage) {
  image.onload?.();
  image.finish();
  await Promise.resolve();
  await Promise.resolve();
}
it('解码和实际绘制分开确认；连续视图更新撤销旧绘制代次并采用当前 DPR', async () => {
  renderer.update(frame);
  await decode(images[0]!);
  expect(reports.at(-1)).toMatchObject({ decoded: true, ready: false });
  tick();
  renderer.update({ ...frame, dpr: 2, viewportWidth: 600 });
  tick();
  tick();
  expect(reports.at(-1)).toMatchObject({
    generation: 2,
    paintedGeneration: 2,
    ready: true,
    surface: { width: 1200, height: 600, configuredDpr: 2 },
  });
  expect(images).toHaveLength(1);
});
it('旧图片迟到解码不能覆盖新文档；销毁释放图像、Stage 和待执行回调', async () => {
  renderer.update(frame);
  images[0]!.onload?.();
  renderer.update({ ...frame, url: 'blob:b' });
  images[0]!.finish();
  await Promise.resolve();
  expect(reports.at(-1)?.decoded).toBe(false);
  expect(images[0]!.src).toBe('');
  await decode(images[1]!);
  tick();
  renderer.dispose();
  const count = reports.length;
  tick();
  expect(reports).toHaveLength(count);
  expect(resource.destroyed).toBe(resource.created);
  expect(images[1]!.src).toBe('');
  expect(frames.size).toBe(0);
});
it('DPR 上升伴随视口缩小时不按旧 CSS 尺寸临时分配超限表面', async () => {
  renderer.update({ ...frame, viewportWidth: 7900, viewportHeight: 200, dpr: 1 });
  await decode(images[0]!);
  tick();
  tick();
  renderer.update({ ...frame, viewportWidth: 1800, viewportHeight: 200, dpr: 4 });
  tick();
  tick();
  expect(reports.at(-1)).toMatchObject({ ready: true, surface: { width: 7200, height: 800 } });
  expect(resource.maxWidth).toBeLessThanOrEqual(8192);
});
it('超预算视口显式失败且不分配 Stage；源清空后无图像或后备表面', async () => {
  expect(validSurface(800, 600, 2)).toBe(true);
  expect(validSurface(10000, 10000, 2)).toBe(false);
  renderer.update({ ...frame, viewportWidth: 10000 });
  await decode(images[0]!);
  tick();
  expect(reports.at(-1)).toMatchObject({ ready: false, decoded: false, surface: null });
  expect(reports.at(-1)?.error).toContain('预算');
  expect(resource.created).toBe(0);
  renderer.update({ ...frame, url: null });
  expect(reports.at(-1)).toMatchObject({
    ready: false,
    decoded: false,
    error: null,
    surface: null,
  });
});

it('销毁只释放持有资源，迟到 decode 仍计为在途直到真实完成；重复销毁幂等', async () => {
  const before = readPreviewResources();
  renderer.update(frame);
  images[0]!.onload?.();
  renderer.dispose();
  renderer.dispose();
  const closing = readPreviewResources();
  expect(closing.renderers.live).toBe(before.renderers.live - 1);
  expect(closing.images.live).toBe(before.images.live);
  expect(closing.decodes.live).toBe(before.decodes.live + 1);
  images[0]!.finish();
  await Promise.resolve();
  await Promise.resolve();
  expect(readPreviewResources().decodes.live).toBe(before.decodes.live);
  expect(frames.size).toBe(0);
});

it('绘制回调和表面所有权随换源及销毁平衡，不能仅以 DOM 消失推断释放', async () => {
  const before = readPreviewResources();
  renderer.update(frame);
  await decode(images[0]!);
  tick();
  expect(readPreviewResources().stages.live).toBe(before.stages.live + 1);
  expect(readPreviewResources().frames.live).toBe(before.frames.live + 1);
  renderer.update({ ...frame, url: null });
  renderer.dispose();
  const after = readPreviewResources();
  for (const kind of ['stages', 'images', 'frames', 'decodes'] as const)
    expect(after[kind].live).toBe(before[kind].live);
});
