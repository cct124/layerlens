import { shallowRef } from 'vue';
import type { UiPreviewResources } from '../../shared/api/devGenerated';

type Resource = Exclude<keyof UiPreviewResources, 'canvasElements' | 'canvasPixels'>;
const counts = {
  renderers: { created: 0, released: 0, live: 0 },
  stages: { created: 0, released: 0, live: 0 },
  images: { created: 0, released: 0, live: 0 },
  objectUrls: { created: 0, released: 0, live: 0 },
  reads: { created: 0, released: 0, live: 0 },
  decodes: { created: 0, released: 0, live: 0 },
  frames: { created: 0, released: 0, live: 0 },
};
export const previewResourceVersion = shallowRef(0);

/** 在实际取得／释放所有权处计数；不保留对象，也不把解引用当成浏览器已回收。 */
export function ownPreviewResource(kind: Resource): () => void {
  const counter = counts[kind];
  counter.created++;
  counter.live++;
  previewResourceVersion.value++;
  let released = false;
  return () => {
    if (released) return;
    released = true;
    counter.released++;
    counter.live--;
    previewResourceVersion.value++;
  };
}

/** 只读复制；Canvas 数据取自仍挂载的正式表面，不包含离屏内部表面或显存。 */
export function readPreviewResources(): UiPreviewResources {
  const canvases = [...document.querySelectorAll<HTMLCanvasElement>('.composite-surface canvas')];
  return {
    ...structuredClone(counts),
    canvasElements: canvases.length,
    canvasPixels: canvases.reduce((pixels, canvas) => pixels + canvas.width * canvas.height, 0),
  };
}
