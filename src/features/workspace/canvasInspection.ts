import type { LayerSummary } from '../../shared/api/generated';
import type { Point } from './regionDraft';

/** 只接受同修订的完整摘要，保持核心从上到下的堆叠顺序。未加载完不能命中。 */
export function inspectionLayers(
  layers: readonly LayerSummary[] | null,
): readonly LayerSummary[] | null {
  if (!layers) return null;
  const visible = new Map<number, boolean>();
  return layers.filter((layer) => {
    const effective =
      layer.visible &&
      layer.effectiveVisible &&
      layer.opacity > 0 &&
      (layer.parentId === null || visible.get(layer.parentId) === true);
    visible.set(layer.id, effective);
    const b = layer.bounds;
    return (
      effective &&
      layer.kind !== 'group' &&
      [b.x, b.y, b.width, b.height].every(Number.isFinite) &&
      b.width > 0 &&
      b.height > 0
    );
  });
}

/** 几何边界命中；透明像素、蒙版和复杂效果尚不具备像素精度。 */
export function hitLayers(
  layers: readonly LayerSummary[],
  point: Point,
  width: number,
  height: number,
): number[] {
  if (point.x < 0 || point.y < 0 || point.x >= width || point.y >= height) return [];
  return layers
    .filter(
      ({ bounds: b }) =>
        point.x >= b.x && point.y >= b.y && point.x < b.x + b.width && point.y < b.y + b.height,
    )
    .map((layer) => layer.id);
}
