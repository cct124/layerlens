import Konva from 'konva';
import type { Assets } from '../resources';
import { uniformRun } from '../text';
import { cssColor } from '../scene';
import type { Renderer, View } from '../renderer';

export async function create(host: HTMLDivElement, assets: Assets, view: View): Promise<Renderer> {
  // 固定 10.9.0 的现代字母基线模式，公共 measureSize 返回其实际使用的字体度量。
  Konva.legacyTextRendering = false;
  const alignBaseline = (node: Konva.Text, baseline: number) => {
    const metrics = node.measureSize('M');
    const ascent = metrics.fontBoundingBoxAscent ?? metrics.actualBoundingBoxAscent;
    const descent = metrics.fontBoundingBoxDescent ?? metrics.actualBoundingBoxDescent;
    node.y(baseline - ((ascent - descent) / 2 + (node.lineHeight() * node.fontSize()) / 2));
  };
  const stage = new Konva.Stage({ container: host, width: view.width, height: view.height });
  const layer = new Konva.Layer({ listening: false, imageSmoothingEnabled: false });
  const root = new Konva.Group();
  layer.add(root);
  stage.add(layer);
  layer.getCanvas().setPixelRatio(view.dpr);
  let count = 0,
    groups = 0,
    disposed = false,
    nativeTextLayers = 0,
    sharedTextLayers = 0;
  try {
    for (const item of [...assets.layers].reverse()) {
      const l = item.layer,
        c = l.content,
        b = l.bounds;
      if (!l.visible || !l.opacity || item.problem) continue;
      const group = new Konva.Group({ opacity: l.opacity / 255, listening: false });
      groups++;
      if (c.kind === 'path')
        for (const p of item.paths) {
          group.add(
            new Konva.Path({
              data: p.data,
              fill: cssColor(c.color),
              fillRule: p.evenOdd ? 'evenodd' : 'nonzero',
            }),
          );
          count++;
        }
      else if (c.kind === 'bitmap' && item.image) {
        group.add(
          new Konva.Image({ image: item.image, x: b.x, y: b.y, width: b.width, height: b.height }),
        );
        count++;
      } else if (c.kind === 'text' && c.data.transform) {
        group.setAttrs(new Konva.Transform([...c.data.transform]).decompose());
        const native = view.textMode === 'native' ? uniformRun(item.runs) : null;
        if (native) {
          const box = c.textBox;
          const text = new Konva.Text({
            text: native.text,
            x: box?.x ?? 0,
            y: box?.y ?? 0,
            ...(box ? { width: box.width } : {}),
            fontSize: native.size,
            fontFamily: native.family,
            fill: native.color,
            lineHeight: native.leading / native.size,
            padding: 0,
            wrap: 'word',
          });
          if (!box) alignBaseline(text, 0);
          group.add(text);
          count++;
          nativeTextLayers++;
        } else {
          sharedTextLayers++;
          for (const span of item.spans) {
            const text = new Konva.Text({
              text: span.text,
              x: span.x,
              y: 0,
              fontSize: span.size,
              fontFamily: span.family,
              fill: span.color,
              padding: 0,
              lineHeight: 1,
            });
            alignBaseline(text, span.y);
            group.add(text);
            count++;
          }
        }
      }
      root.add(group);
    }
    return {
      backend: 'canvas2d',
      nodeCount: count,
      resourceCount: count + groups + 3,
      nativeTextLayers,
      sharedTextLayers,
      async draw(v) {
        if (disposed) throw new Error('Konva 已释放');
        const started = performance.now();
        root.position({ x: v.panX, y: v.panY });
        root.scale({ x: v.zoom, y: v.zoom });
        stage.draw();
        return performance.now() - started;
      },
      canvas() {
        return layer.getCanvas()._canvas;
      },
      dispose() {
        if (disposed) return;
        disposed = true;
        stage.destroy();
      },
    };
  } catch (error) {
    stage.destroy();
    throw error;
  }
}
