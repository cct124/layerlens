import type { UiPanels, UiPanelScroll, UiRect } from '../../shared/api/devGenerated';
import { PANEL_LAYOUT_KEY, parsePanelPreferences, SPLITTER_SIZE } from './panelLayout';

const bounds = (element: Element): UiRect => {
  const { x, y, width, height } = element.getBoundingClientRect();
  return { x, y, width, height };
};
function scroll(target: string): UiPanelScroll | null {
  const element = document.querySelector<HTMLElement>('[data-dev-target="' + target + '"]');
  if (!element || element.getClientRects().length === 0) return null;
  const rows = [...element.querySelectorAll<HTMLElement>('.tree-row')];
  const id = (row?: HTMLElement) => {
    const target = row?.querySelector<HTMLElement>('.tree-name')?.dataset.devTarget;
    return target ? Number(target.split(':')[1]) : null;
  };
  return {
    rect: bounds(element),
    scrollTop: element.scrollTop,
    scrollHeight: element.scrollHeight,
    clientHeight: element.clientHeight,
    renderedRows: rows.length,
    firstLayerId: id(rows[0]),
    lastLayerId: id(rows.at(-1)),
    textLength: element.textContent?.length ?? 0,
  };
}

/** 仅由已启用的开发观测桥调用；读取真实几何和特定布局偏好，不改变 DOM 或存储。 */
export function observePanels(): UiPanels | null {
  const width = document.querySelector<HTMLElement>('[data-dev-target="panel:width"]');
  const height = document.querySelector<HTMLElement>('[data-dev-target="panel:height"]');
  const sidebar = document.querySelector('.sidebar');
  const stack = document.querySelector('.sidebar-stack');
  const inspector = document.querySelector('[data-panel-id="workspace-inspector"]');
  const resources = document.querySelector('[data-panel-id="workspace-resources"]');
  if (!width || !height || !sidebar || !stack || !inspector || !resources) return null;
  let savedSidebarWidth = null,
    savedInspectorPercent = null,
    preferenceError = false;
  try {
    const raw = localStorage.getItem(PANEL_LAYOUT_KEY);
    if (raw !== null) {
      const saved = parsePanelPreferences(raw);
      savedSidebarWidth = saved.sidebarWidth;
      savedInspectorPercent = saved.inspectorPercent;
    }
  } catch {
    preferenceError = true;
  }
  const tree = document.querySelector<HTMLElement>('.layer-tree');
  const value = (target: string) =>
    document.querySelector<HTMLInputElement>('[data-dev-target="' + target + '"]')?.value ?? '';
  return {
    sidebarWidth: bounds(sidebar).width,
    inspectorHeight: bounds(inspector).height,
    keyboardEvents:
      Number(width.dataset.panelKeyEvents ?? 0) + Number(height.dataset.panelKeyEvents ?? 0),
    resourcesHeight: bounds(resources).height,
    focusedHandle:
      document.activeElement === width
        ? 'width'
        : document.activeElement === height
          ? 'height'
          : null,
    inspectorPercent:
      (bounds(inspector).height / Math.max(1, bounds(stack).height - SPLITTER_SIZE)) * 100,
    widthHandle: bounds(width),
    heightHandle: bounds(height),
    dragging: [width, height].some((e) => e.dataset.resizeHandleState === 'drag'),
    savedSidebarWidth,
    savedInspectorPercent,
    preferenceError,
    treeCount: Number(tree?.dataset.layerCount ?? 0),
    treeLoading: tree?.dataset.loading === 'true',
    tree: scroll('panel:tree'),
    properties: scroll('panel:details'),
    search: value('layers:search'),
    taskName: value('task:name'),
  };
}
