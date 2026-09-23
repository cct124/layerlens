/** 仅调整选框草稿的边缘吸附；不计算区域命中，不创建核心选区或任务。 */
import type { LayerSummary, SelectionBounds } from '../../shared/api/generated';
import { updateRegion } from './regionDraft';
import type { Point, RegionDraft } from './regionDraft';

export const SNAP_ENTER_CSS_PX = 6;
export const SNAP_RELEASE_CSS_PX = 10;
type Axis = 'x' | 'y';
type ProbeId = 'start' | 'end' | 'moving';
export interface SnapEdge {
  readonly value: number;
  readonly layerId: number | null;
  readonly label: string;
}
export interface SnapIndex {
  readonly x: readonly SnapEdge[];
  readonly y: readonly SnapEdge[];
  readonly layersReady: boolean;
}
export interface SnapGuide {
  readonly target: SnapEdge;
  readonly probe: ProbeId;
}
export interface SnapGuides {
  x: SnapGuide | null;
  y: SnapGuide | null;
}
interface Probe {
  id: ProbeId;
  value: number;
  rawValue: number;
  min: number;
  max: number;
}
interface AxisMotion {
  fixed: number | null;
  probes: Probe[];
}

function priority(a: SnapEdge, b: SnapEdge) {
  return (a.layerId ?? -1) - (b.layerId ?? -1) || a.value - b.value;
}
function sortedEdges(edges: SnapEdge[]): readonly SnapEdge[] {
  // 同坐标只保留稳定代表，避免重叠图层造成逐帧线性扫描或目标标签抖动。
  return edges
    .sort((a, b) => a.value - b.value || priority(a, b))
    .filter((edge, index, all) => index === 0 || edge.value !== all[index - 1]!.value);
}
/** 使用完整、已校验的当前修订摘要建立有序索引；null 表示列表未完整，只吸附画布。 */
export function buildSnapIndex(
  width: number,
  height: number,
  layers: readonly LayerSummary[] | null = null,
): SnapIndex {
  const x: SnapEdge[] = [
    { value: 0, layerId: null, label: '画布左边' },
    { value: width, layerId: null, label: '画布右边' },
  ];
  const y: SnapEdge[] = [
    { value: 0, layerId: null, label: '画布上边' },
    { value: height, layerId: null, label: '画布下边' },
  ];
  for (const layer of layers ?? []) {
    const b = layer.bounds;
    if (
      !layer.effectiveVisible ||
      layer.kind === 'group' ||
      ![b.x, b.y, b.width, b.height].every(Number.isFinite) ||
      b.width <= 0 ||
      b.height <= 0 ||
      b.x >= width ||
      b.y >= height ||
      b.x + b.width <= 0 ||
      b.y + b.height <= 0
    )
      continue;
    const add = (edges: SnapEdge[], value: number, max: number, side: string) => {
      if (value >= 0 && value <= max)
        edges.push({ value, layerId: layer.id, label: `${layer.name} · 几何${side}` });
    };
    // DTO 当前只有几何边界，不能将其宣称为包含效果的视觉边界。
    add(x, b.x, width, '左边');
    add(x, b.x + b.width, width, '右边');
    add(y, b.y, height, '上边');
    add(y, b.y + b.height, height, '下边');
  }
  return { x: sortedEdges(x), y: sortedEdges(y), layersReady: layers !== null };
}

function axisMotion(
  draft: RegionDraft,
  point: Point,
  axis: Axis,
  extent: number,
): AxisMotion | null {
  const length = axis === 'x' ? 'width' : 'height';
  const startSide = axis === 'x' ? 'w' : 'n';
  const endSide = axis === 'x' ? 'e' : 's';
  const { action, original, bounds, start } = draft;
  if (action === 'move')
    return {
      fixed: null,
      probes: [
        {
          id: 'start',
          value: bounds[axis],
          rawValue: (original?.[axis] ?? bounds[axis]) + point[axis] - start[axis],
          min: 0,
          max: extent - bounds[length],
        },
        {
          id: 'end',
          value: bounds[axis] + bounds[length],
          rawValue: (original?.[axis] ?? bounds[axis]) + point[axis] - start[axis] + bounds[length],
          min: bounds[length],
          max: extent,
        },
      ],
    };
  let fixed: number;
  let moving: number;
  if (action === 'create') {
    fixed = start[axis];
    moving = point[axis];
  } else if (original && (action.includes(startSide) || action.includes(endSide))) {
    const fromStart = action.includes(startSide);
    fixed = original[axis] + (fromStart ? original[length] : 0);
    moving = original[axis] + (fromStart ? 0 : original[length]) + point[axis] - start[axis];
  } else return null;
  return {
    fixed,
    probes: [
      {
        id: 'moving',
        value: Math.max(0, Math.min(extent, moving)),
        rawValue: moving,
        min: 0,
        max: extent,
      },
    ],
  };
}
function lowerBound(edges: readonly SnapEdge[], value: number): number {
  let low = 0,
    high = edges.length;
  while (low < high) {
    const mid = Math.floor((low + high) / 2);
    if (edges[mid]!.value < value) low = mid + 1;
    else high = mid;
  }
  return low;
}
function snapAxis(
  motion: AxisMotion | null,
  edges: readonly SnapEdge[],
  previous: SnapGuide | null,
  zoom: number,
): SnapGuide | null {
  if (!motion) return null;
  const valid = (target: SnapEdge, probe: Probe) =>
    target.value >= probe.min && target.value <= probe.max && target.value !== motion.fixed;
  const held = previous && motion.probes.find((probe) => probe.id === previous.probe);
  if (
    previous &&
    held &&
    valid(previous.target, held) &&
    // 脱离使用未裁剪的位置，否则画布边缘附近的图层会一直粘住向外拖动的指针。
    Math.abs(previous.target.value - held.rawValue) * zoom <= SNAP_RELEASE_CSS_PX
  )
    return previous;

  let best: SnapGuide | null = null;
  let distance = Infinity;
  for (const probe of motion.probes) {
    const insertion = lowerBound(edges, probe.value);
    for (const direction of [-1, 1]) {
      let index = direction < 0 ? insertion - 1 : insertion;
      while (index >= 0 && index < edges.length) {
        const target = edges[index]!;
        const gap = Math.abs(target.value - probe.value) * zoom;
        if (gap > SNAP_ENTER_CSS_PX || target.value < probe.min || target.value > probe.max) break;
        if (valid(target, probe)) {
          // 浮点坐标的同距容差也使用 CSS 像素；其后画布、图层 ID、坐标和探针顺序稳定裁决。
          if (
            gap < distance - 1e-9 ||
            (Math.abs(gap - distance) <= 1e-9 && best && priority(target, best.target) < 0)
          ) {
            best = { target, probe: probe.id };
            distance = gap;
          }
          break;
        }
        // 唯一可能跳过的区间内目标是固定对边，不能把有效区域吸成零面积。
        index += direction;
      }
    }
  }
  return best;
}
function applyAxis(bounds: SelectionBounds, axis: Axis, motion: AxisMotion, guide: SnapGuide) {
  const length = axis === 'x' ? 'width' : 'height';
  if (motion.fixed === null) {
    bounds[axis] = guide.target.value - (guide.probe === 'end' ? bounds[length] : 0);
  } else {
    bounds[axis] = Math.min(motion.fixed, guide.target.value);
    bounds[length] = Math.abs(guide.target.value - motion.fixed);
  }
}
/** 从未吸附的指针增量重算，避免吸附误差积累；previous 必须来自同一次手势的固定索引。 */
export function snapRegion(
  draft: RegionDraft,
  point: Point,
  width: number,
  height: number,
  zoom: number,
  index: SnapIndex,
  previous: SnapGuides,
  enabled = true,
): { draft: RegionDraft; guides: SnapGuides } {
  const raw = updateRegion(draft, point, width, height);
  const guides: SnapGuides = { x: null, y: null };
  if (!enabled || !Number.isFinite(zoom) || zoom <= 0) return { draft: raw, guides };
  for (const axis of ['x', 'y'] as const) {
    const motion = axisMotion(raw, point, axis, axis === 'x' ? width : height);
    const guide = snapAxis(motion, index[axis], previous[axis], zoom);
    if (motion && guide) {
      guides[axis] = guide;
      applyAxis(raw.bounds, axis, motion, guide);
    }
  }
  return { draft: raw, guides };
}
