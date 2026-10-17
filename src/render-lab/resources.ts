import type { RenderScene, RenderLayer } from './generated';
import { pathData } from './scene';
import { FONT_SHA, loadFonts } from './fonts';
import { prepareText, type TextSpan, type TextRun } from './text';

export type PreparedLayer = {
  layer: RenderLayer;
  paths: { data: string; path: Path2D; evenOdd: boolean }[];
  spans: TextSpan[];
  runs: TextRun[];
  problem?: string;
  textRasterReason?: string;
  image?: HTMLImageElement;
  url?: string;
  alpha?: Uint8Array;
};
export type Assets = {
  fontHash: string;
  layers: PreparedLayer[];
  referenceUrl: string;
  limitations: string[];
  encodedBytes: number;
  alphaBytes: number;
  dispose(): void;
};

/** 资源加载完成才允许绘制；失败回收已创建的 Blob 与字体。 */
export async function prepareAssets(
  scene: RenderScene,
  read?: (id: number | null) => Promise<ArrayBuffer>,
  signal?: AbortSignal,
): Promise<Assets> {
  const urls: string[] = [];
  const faces: FontFace[] = [];
  const layers: PreparedLayer[] = [];
  let encodedBytes = 0,
    alphaBytes = 0;
  const dispose = () => {
    for (const layer of layers) {
      if (layer.image) layer.image.src = '';
      delete layer.alpha;
      delete layer.image;
    }
    urls.splice(0).forEach(URL.revokeObjectURL);
    faces.splice(0).forEach((face) => document.fonts.delete(face));
  };
  const pngUrl = async (inline: number[], id: number | null) => {
    signal?.throwIfAborted();
    const bytes = inline.length ? new Uint8Array(inline).buffer : await read?.(id);
    signal?.throwIfAborted();
    if (!bytes || (encodedBytes += bytes.byteLength) > 128 * 1024 * 1024)
      throw new Error('实验资源累计超过 128 MiB 或缺少读取器');
    const url = URL.createObjectURL(new Blob([bytes], { type: 'image/png' }));
    urls.push(url);
    return url;
  };
  const bitmap = async (item: PreparedLayer, inline: number[]) => {
    const layer = item.layer;
    item.url = await pngUrl(inline, layer.id);
    item.image = new Image();
    item.image.src = item.url;
    await item.image.decode();
    signal?.throwIfAborted();
    if (
      item.image.naturalWidth !== layer.bounds.width ||
      item.image.naturalHeight !== layer.bounds.height
    )
      throw new Error('图层 PNG 尺寸不符');
    const canvas = document.createElement('canvas');
    canvas.width = layer.bounds.width;
    canvas.height = layer.bounds.height;
    try {
      const ctx = canvas.getContext('2d', { willReadFrequently: true });
      if (!ctx) throw new Error('无法读取 alpha');
      ctx.drawImage(item.image, 0, 0);
      const rgba = ctx.getImageData(0, 0, canvas.width, canvas.height).data;
      item.alpha = new Uint8Array(canvas.width * canvas.height);
      for (let i = 0; i < item.alpha.length; i++) item.alpha[i] = rgba[i * 4 + 3]!;
      alphaBytes += item.alpha.length;
      if (alphaBytes > 96 * 1024 * 1024) throw new Error('alpha 缓存超过实验预算');
    } finally {
      canvas.width = canvas.height = 0;
    }
  };
  try {
    const names = new Set(['NotoSans-Regular']);
    for (const l of scene.layers)
      if (l.content.kind === 'text')
        for (const r of l.content.data.styleRuns ?? [])
          if (r.style.font) names.add(r.style.font.value.name);
    const hashes = await loadFonts(names, faces);
    const fontHash =
      hashes.length === 1
        ? FONT_SHA
        : Array.from(
            new Uint8Array(
              await crypto.subtle.digest('SHA-256', new TextEncoder().encode(hashes.join(':'))),
            ),
            (n) => n.toString(16).padStart(2, '0'),
          ).join('');
    const measure = document.createElement('canvas').getContext('2d');
    if (!measure) throw new Error('无法建立文字度量上下文');
    const limitations = [
      ...scene.warnings,
      '共享模式为分段与逐字换行基线；原生模式的实际覆盖由 Konva 适配器报告。',
      '命中使用共享路径／像素几何；文字为源边界候选，不宣称引擎原生命中。',
    ];
    for (const layer of scene.layers) {
      signal?.throwIfAborted();
      const item: PreparedLayer = { layer, paths: [], spans: [], runs: [] };
      layers.push(item);
      const c = layer.content;
      if (!layer.visible || !layer.opacity) {
        continue;
      }
      if (c.kind === 'bitmap') {
        await bitmap(item, c.png);
      } else if (c.kind === 'path')
        item.paths = c.paths.map((p) => ({
          data: pathData(p),
          path: new Path2D(pathData(p)),
          evenOdd: p.evenOdd,
        }));
      else if (c.kind === 'text') {
        try {
          const prepared = prepareText(c.data, c.textBox, measure);
          item.spans = prepared.spans;
          item.runs = prepared.runs;
        } catch (error) {
          item.problem = error instanceof Error ? error.message : String(error);
        }
        if (item.problem && read && layer.compositionWarning !== null) {
          // 仅显式源图层诊断允许缓存像素回退；保留原因，不能把它计为原生文字。
          item.textRasterReason = item.problem;
          await bitmap(item, []);
          item.layer = { ...layer, content: { kind: 'bitmap', png: [] } };
          delete item.problem;
        }
      } else if (c.kind === 'unsupported') item.problem = c.reason;
    }
    const problems = new Map<string, number>();
    for (const l of layers)
      if (l.problem || l.textRasterReason) {
        const reason = l.problem ?? '文字像素回退：' + l.textRasterReason;
        problems.set(reason, (problems.get(reason) ?? 0) + 1);
      }
    limitations.push(
      ...Array.from(problems, ([reason, count]) => count + ' 层：' + reason).slice(0, 48),
    );
    return {
      fontHash,
      layers,
      referenceUrl: await pngUrl(scene.referencePng, null),
      encodedBytes,
      alphaBytes,
      limitations,
      dispose,
    };
  } catch (error) {
    dispose();
    throw error;
  }
}

/** 基于源几何和 alpha 命中；与绘制节点的默认事件规则独立。 */
export function hitCandidates(
  assets: Assets,
  x: number,
  y: number,
): { id: number; precision: 'pixel' | 'path' | 'bounds' }[] {
  const ctx = document.createElement('canvas').getContext('2d');
  if (!ctx) return [];
  return assets.layers.flatMap<{ id: number; precision: 'path' | 'pixel' | 'bounds' }>((item) => {
    const l = item.layer,
      c = l.content,
      b = l.bounds;
    if (item.problem || !l.visible || !l.opacity || c.kind === 'group' || c.kind === 'unsupported')
      return [];
    if (c.kind === 'path')
      return item.paths.some((p) =>
        ctx.isPointInPath(p.path, x, y, p.evenOdd ? 'evenodd' : 'nonzero'),
      )
        ? [{ id: l.id, precision: 'path' as const }]
        : [];
    if (x < b.x || y < b.y || x >= b.x + b.width || y >= b.y + b.height) return [];
    if (c.kind === 'bitmap')
      return (item.alpha?.[Math.floor(y - b.y) * b.width + Math.floor(x - b.x)] ?? 0) > 0
        ? [{ id: l.id, precision: 'pixel' as const }]
        : [];
    return [{ id: l.id, precision: 'bounds' as const }];
  });
}
