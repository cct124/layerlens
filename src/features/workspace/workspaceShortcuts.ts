/** 工作区级快捷键只路由操作；清空、忙碌及上下文确认仍由既有应用入口维护。 */
export type WorkspaceShortcut = 'open' | 'pan' | 'region' | 'fit' | 'actual';

export function workspaceShortcut(event: KeyboardEvent): WorkspaceShortcut | null {
  if (event.defaultPrevented || event.repeat || event.isComposing || event.altKey || event.shiftKey)
    return null;
  if (
    event.target instanceof Element &&
    event.target.closest(
      'input, textarea, select, [contenteditable]:not([contenteditable="false"])',
    )
  )
    return null;
  const key = event.key.toLowerCase();
  if (event.ctrlKey || event.metaKey) {
    if (key === 'o') return 'open';
    if (key === '0') return 'fit';
    if (key === '1') return 'actual';
    return null;
  }
  if (key === 'v') return 'pan';
  if (key === 'm') return 'region';
  return null;
}
