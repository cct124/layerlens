/** 面板布局仅是本机视图偏好，尺寸均为 CSS px，不参与核心选区或任务授权。 */
export interface PanelPreferences {
  version: 1;
  sidebarWidth: number;
  inspectorPercent: number;
}

export const PANEL_LAYOUT_KEY = 'layerlens:panel-layout:v1';
export const SPLITTER_SIZE = 8;
export const defaultPanelPreferences = (): PanelPreferences => ({
  version: 1,
  sidebarWidth: 320,
  inspectorPercent: 36,
});
const clamp = (value: number, min: number, max: number) => Math.min(max, Math.max(min, value));

/** 本地存储也需要边界校验；不兼容或损坏的记录由调用方提示后回退。 */
export function parsePanelPreferences(raw: string | null): PanelPreferences {
  if (raw === null) return defaultPanelPreferences();
  const value: unknown = JSON.parse(raw);
  if (
    typeof value !== 'object' ||
    value === null ||
    !('version' in value) ||
    value.version !== 1 ||
    !('sidebarWidth' in value) ||
    typeof value.sidebarWidth !== 'number' ||
    !Number.isFinite(value.sidebarWidth) ||
    value.sidebarWidth < 260 ||
    value.sidebarWidth > 560 ||
    !('inspectorPercent' in value) ||
    typeof value.inspectorPercent !== 'number' ||
    !Number.isFinite(value.inspectorPercent) ||
    value.inspectorPercent <= 0 ||
    value.inspectorPercent >= 100
  )
    throw new Error('布局记录格式或版本无效');
  return { version: 1, sidebarWidth: value.sidebarWidth, inspectorPercent: value.inspectorPercent };
}

/** 扣除真实分隔条宽度后换算比例，避免库按整个容器换算 px 时侵占最小尺寸。 */
export function sidebarLayout(containerWidth: number, preferredWidth: number) {
  const available = Math.max(1, containerWidth - SPLITTER_SIZE);
  // 正常窗口保留 400 px 画布；极小浏览器预览按比例退让，不产生相互矛盾的约束。
  const min = Math.min(260, (available * 260) / 660);
  const max = Math.min(560, available - Math.min(400, (available * 400) / 660));
  const width = clamp(preferredWidth, min, max);
  return {
    available,
    minPercent: (min / available) * 100,
    maxPercent: (max / available) * 100,
    percent: (width / available) * 100,
  };
}

/** 上下分区保存比例，正常窗口分别保留 140／180 CSS px 内容高度。 */
export function inspectorLayout(containerHeight: number, preferredPercent: number) {
  const available = Math.max(1, containerHeight - SPLITTER_SIZE);
  const min = Math.min(140, (available * 140) / 320);
  const max = available - Math.min(180, (available * 180) / 320);
  const minPercent = (min / available) * 100;
  const maxPercent = (max / available) * 100;
  return { minPercent, maxPercent, percent: clamp(preferredPercent, minPercent, maxPercent) };
}
