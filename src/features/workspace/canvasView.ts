/** 缩放为原稿像素到界面像素的比例；平移为画布中心的界面像素偏移。 */
export interface CanvasView {
  mode: 'fit' | 'manual';
  zoom: number;
  x: number;
  y: number;
}
export const initialView = (): CanvasView => ({ mode: 'fit', zoom: 1, x: 0, y: 0 });
export function fitZoom(
  width: number,
  height: number,
  viewportWidth: number,
  viewportHeight: number,
): number {
  return Math.max(0.01, Math.min(1, (viewportWidth - 64) / width, (viewportHeight - 64) / height));
}
export function zoomAround(view: CanvasView, zoom: number, pointX = 0, pointY = 0): CanvasView {
  const next = Math.max(0.01, Math.min(8, zoom));
  const ratio = next / view.zoom;
  return {
    mode: 'manual',
    zoom: next,
    x: pointX - (pointX - view.x) * ratio,
    y: pointY - (pointY - view.y) * ratio,
  };
}
