import { expect, it } from 'vitest';
import { workspaceShortcut } from './workspaceShortcuts';

it('仅映射已实现的工具、文件与视图快捷键', () => {
  for (const [key, action] of [
    ['v', 'pan'],
    ['m', 'region'],
    ['M', 'region'],
  ] as const)
    expect(workspaceShortcut(new KeyboardEvent('keydown', { key }))).toBe(action);
  for (const [key, action] of [
    ['o', 'open'],
    ['0', 'fit'],
    ['1', 'actual'],
  ] as const)
    expect(workspaceShortcut(new KeyboardEvent('keydown', { key, ctrlKey: true }))).toBe(action);
  expect(workspaceShortcut(new KeyboardEvent('keydown', { key: 'Escape' }))).toBeNull();
  expect(workspaceShortcut(new KeyboardEvent('keydown', { key: 'v', ctrlKey: true }))).toBeNull();
});

it('忽略重复、输入法、编辑控件及不支持的组合键', () => {
  for (const flag of ['repeat', 'isComposing', 'altKey', 'shiftKey'])
    expect(workspaceShortcut(new KeyboardEvent('keydown', { key: 'v', [flag]: true }))).toBeNull();
  for (const html of [
    '<input>',
    '<textarea></textarea>',
    '<select></select>',
    '<div contenteditable><span></span></div>',
  ]) {
    const root = document.createElement('div');
    root.innerHTML = html;
    const target = root.querySelector('span') ?? root.firstElementChild!;
    target.addEventListener('keydown', (event) => {
      if (!(event instanceof KeyboardEvent)) throw new Error('预期键盘事件');
      expect(workspaceShortcut(event)).toBeNull();
    });
    target.dispatchEvent(new KeyboardEvent('keydown', { key: 'v' }));
    target.dispatchEvent(new KeyboardEvent('keydown', { key: '0', ctrlKey: true }));
  }
  const handled = new KeyboardEvent('keydown', { key: 'm', cancelable: true });
  handled.preventDefault();
  expect(workspaceShortcut(handled)).toBeNull();
});
