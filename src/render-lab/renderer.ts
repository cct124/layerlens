import type { Assets } from './resources';
export type TextMode = 'shared' | 'native';
export type View = {
  width: number;
  height: number;
  dpr: number;
  zoom: number;
  panX: number;
  panY: number;
  textMode: TextMode;
};
export type Renderer = {
  backend: string;
  nodeCount: number;
  resourceCount: number;
  nativeTextLayers: number;
  sharedTextLayers: number;
  /** 返回同步 API 提交耗时；Promise 完成表示引擎就绪，公共读回屏障由调用方执行。 */
  draw(view: View): Promise<number>;
  canvas(): HTMLCanvasElement;
  dispose(): void;
};
export async function createRenderer(
  host: HTMLDivElement,
  assets: Assets,
  view: View,
): Promise<Renderer> {
  return (await import('./renderers/konva')).create(host, assets, view);
}
/** 等待浏览器调度点供观测截图；不是物理屏幕首帧计时。 */
export const nextPaint = () =>
  new Promise<void>((resolve) =>
    requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
  );
