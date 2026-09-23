/** 矩形草稿纯逻辑。指针使用 CSS 像素，矩形使用 PSD 文档像素，不乘设备像素比。 */
import type { SelectionBounds } from '../../shared/api/generated';
import type { CanvasView } from './canvasView';

export type CanvasTool = 'pan' | 'region';
export type RegionHandle = 'n' | 'ne' | 'e' | 'se' | 's' | 'sw' | 'w' | 'nw';
export type RegionAction = 'create' | 'move' | RegionHandle;
export interface Point {
  x: number;
  y: number;
}
export interface CanvasGeometry {
  width: number;
  height: number;
  viewportWidth: number;
  viewportHeight: number;
  view: CanvasView;
}
export interface RegionDraft {
  action: RegionAction;
  start: Point;
  original: SelectionBounds | null;
  bounds: SelectionBounds;
}
export const MIN_REGION_DRAG_CSS_PX = 3;

/** 将相对视口左上角的 CSS 像素位置转换为文档位置，保留亚像素精度。 */
export function documentPoint(point: Point, canvas: CanvasGeometry): Point {
  return {
    x: (point.x - canvas.viewportWidth / 2 - canvas.view.x) / canvas.view.zoom + canvas.width / 2,
    y: (point.y - canvas.viewportHeight / 2 - canvas.view.y) / canvas.view.zoom + canvas.height / 2,
  };
}
/** 文档位置到视口 CSS 像素位置，与 documentPoint 互为逆变换。 */
export function screenPoint(point: Point, canvas: CanvasGeometry): Point {
  return {
    x: canvas.viewportWidth / 2 + canvas.view.x + (point.x - canvas.width / 2) * canvas.view.zoom,
    y: canvas.viewportHeight / 2 + canvas.view.y + (point.y - canvas.height / 2) * canvas.view.zoom,
  };
}
const clamp = (value: number, max: number) => Math.max(0, Math.min(max, value));
/** 检查有限位置是否处于闭合文档矩形内；不判断图层命中。 */
export function insideDocument(point: Point, width: number, height: number): boolean {
  return (
    Number.isFinite(point.x) &&
    Number.isFinite(point.y) &&
    point.x >= 0 &&
    point.y >= 0 &&
    point.x <= width &&
    point.y <= height
  );
}
/** 创建本地草稿，保留此前权威范围供取消恢复和无变化判断。 */
export function beginRegion(
  action: RegionAction,
  start: Point,
  original: SelectionBounds | null,
): RegionDraft {
  return {
    action,
    start,
    original,
    bounds: action === 'create' || !original ? { ...start, width: 0, height: 0 } : { ...original },
  };
}

/** 从起始状态计算，避免累计误差；新建和缩放允许反向拖动，移动保持尺寸。 */
export function updateRegion(
  draft: RegionDraft,
  point: Point,
  width: number,
  height: number,
): RegionDraft {
  const { action, original, start } = draft;
  const x = clamp(point.x, width),
    y = clamp(point.y, height);
  if (action === 'move' && original) {
    return {
      ...draft,
      bounds: {
        ...original,
        x: clamp(original.x + point.x - start.x, width - original.width),
        y: clamp(original.y + point.y - start.y, height - original.height),
      },
    };
  }
  let left = start.x,
    right = x,
    top = start.y,
    bottom = y;
  if (action !== 'create' && original) {
    // 控制点有屏幕命中面积，以拖动增量调整原边缘，点击边缘外侧也不会跳动。
    left = action.includes('w') ? clamp(original.x + point.x - start.x, width) : original.x;
    right = action.includes('e')
      ? clamp(original.x + original.width + point.x - start.x, width)
      : original.x + original.width;
    top = action.includes('n') ? clamp(original.y + point.y - start.y, height) : original.y;
    bottom = action.includes('s')
      ? clamp(original.y + original.height + point.y - start.y, height)
      : original.y + original.height;
  }
  return {
    ...draft,
    bounds: {
      x: Math.min(left, right),
      y: Math.min(top, bottom),
      width: Math.abs(right - left),
      height: Math.abs(bottom - top),
    },
  };
}

/** 精确比较文档范围，避免未移动时产生多余版本。 */
export function sameRegion(a: SelectionBounds | null, b: SelectionBounds): boolean {
  return !!a && a.x === b.x && a.y === b.y && a.width === b.width && a.height === b.height;
}
/** 点击、零面积、越界和未变更的草稿都不提交；最小拖动距离按屏幕像素判断。 */
export function finishRegion(
  draft: RegionDraft,
  travel: number,
  width: number,
  height: number,
): SelectionBounds | null {
  const bounds = draft.bounds;
  if (
    travel < MIN_REGION_DRAG_CSS_PX ||
    bounds.width <= 0 ||
    bounds.height <= 0 ||
    !insideDocument(bounds, width, height) ||
    !insideDocument({ x: bounds.x + bounds.width, y: bounds.y + bounds.height }, width, height) ||
    sameRegion(draft.original, bounds)
  )
    return null;
  return bounds;
}
