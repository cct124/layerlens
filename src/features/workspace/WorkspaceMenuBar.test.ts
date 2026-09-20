import { flushPromises, mount } from '@vue/test-utils';
import type { VueWrapper } from '@vue/test-utils';
import { afterEach, expect, it, vi } from 'vitest';
import WorkspaceMenuBar from './WorkspaceMenuBar.vue';
import type { WorkspaceMenu } from './workspaceMenu';

const menus: WorkspaceMenu[] = [
  {
    id: 'file',
    label: '文件',
    accessKey: 'F',
    items: [
      { command: 'open', label: '打开 PSD', shortcut: 'Ctrl+O' },
      { command: 'reload', label: '重新载入', disabled: true },
      { command: 'close', label: '关闭当前文档', separatorBefore: true },
    ],
  },
  {
    id: 'view',
    label: '视图',
    accessKey: 'V',
    items: [
      { command: 'fit', label: '适应窗口' },
      { command: 'actual', label: '100%' },
    ],
  },
  {
    id: 'window',
    label: '窗口',
    accessKey: 'W',
    items: [
      { command: 'properties', label: '属性', checked: true },
      { command: 'document', label: '文档', checked: false },
      { command: 'layers', label: '图层', checked: true, separatorBefore: true },
      { command: 'tasks', label: '任务', checked: false },
    ],
  },
];
let wrapper: VueWrapper | undefined;
let workSurface: HTMLButtonElement | undefined;
function setup() {
  workSurface = document.createElement('button');
  document.body.append(workSurface);
  workSurface.focus();
  wrapper = mount(WorkspaceMenuBar, { props: { menus }, attachTo: document.body });
  return wrapper;
}
afterEach(() => {
  wrapper?.unmount();
  workSurface?.remove();
});

it('点击打开、再次点击收起；禁用项不执行，命令执行后返回工作面', async () => {
  const view = setup();
  await view.get('[aria-label="文件"]').trigger('pointerdown');
  await view.get('[aria-label="文件"]').trigger('click');
  expect(view.get('[aria-label="文件"]').attributes('aria-expanded')).toBe('true');
  expect(document.activeElement).toBe(view.get('[aria-label="打开 PSD"]').element);
  await view.get('[aria-label="重新载入"]').trigger('click');
  expect(view.emitted('command')).toBeUndefined();
  await view.get('[aria-label="打开 PSD"]').trigger('click');
  expect(view.emitted('command')).toEqual([['open']]);
  expect(view.find('[role=menu]').exists()).toBe(false);
  expect(document.activeElement).toBe(workSurface);
  await view.get('[aria-label="文件"]').trigger('click');
  await view.get('[aria-label="文件"]').trigger('click');
  expect(view.find('[role=menu]').exists()).toBe(false);
});

it('方向键跳过禁用项并循环，左右切换菜单，Esc 只关闭菜单且返回标题', async () => {
  const view = setup();
  const bubbled = vi.fn();
  window.addEventListener('keydown', bubbled);
  try {
    await view.get('[aria-label="文件"]').trigger('keydown', { key: 'ArrowDown' });
    await view.get('[aria-label="打开 PSD"]').trigger('keydown', { key: 'ArrowDown' });
    expect(document.activeElement).toBe(view.get('[aria-label="关闭当前文档"]').element);
    await view.get('[aria-label="关闭当前文档"]').trigger('keydown', { key: 'ArrowDown' });
    expect(document.activeElement).toBe(view.get('[aria-label="打开 PSD"]').element);
    await view.get('[aria-label="打开 PSD"]').trigger('keydown', { key: 'End' });
    expect(document.activeElement).toBe(view.get('[aria-label="关闭当前文档"]').element);
    await view.get('[aria-label="关闭当前文档"]').trigger('keydown', { key: 'ArrowRight' });
    expect(document.activeElement).toBe(view.get('[aria-label="适应窗口"]').element);
    await view.get('[aria-label="适应窗口"]').trigger('keydown', { key: 'Escape' });
    expect(view.find('[role=menu]').exists()).toBe(false);
    expect(document.activeElement).toBe(view.get('[aria-label="视图"]').element);
    expect(bubbled).not.toHaveBeenCalled();
    expect(view.emitted('command')).toBeUndefined();
  } finally {
    window.removeEventListener('keydown', bubbled);
  }
});

it('Alt 访问键与菜单悬停切换可用，窗口面板的两组选中状态分组展示', async () => {
  const view = setup();
  window.dispatchEvent(new KeyboardEvent('keydown', { key: 'f', altKey: true, cancelable: true }));
  await flushPromises();
  expect(view.get('[aria-label="文件"]').attributes('aria-expanded')).toBe('true');
  await view.get('[aria-label="打开 PSD"]').trigger('keydown', { key: 'w', altKey: true });
  expect(view.get('[aria-label="窗口"]').attributes('aria-expanded')).toBe('true');
  expect(view.findAll('.menu-popup [role=group]')).toHaveLength(2);
  expect(view.findAll('[role=menuitemradio][aria-checked=true]')).toHaveLength(2);
  await view.findAll('.menu-group')[1]!.trigger('pointerenter');
  expect(view.get('[aria-label="视图"]').attributes('aria-expanded')).toBe('true');
  await view.get('[aria-label="适应窗口"]').trigger('keydown', { key: 'Tab' });
  expect(view.find('[role=menu]').exists()).toBe(false);
});

it('外部点击、焦点离开和窗口失焦收起菜单；卸载移除全局监听', async () => {
  const view = setup();
  await view.get('[aria-label="文件"]').trigger('click');
  document.body.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true }));
  await flushPromises();
  expect(view.find('[role=menu]').exists()).toBe(false);
  await view.get('[aria-label="文件"]').trigger('click');
  workSurface!.focus();
  await flushPromises();
  expect(view.find('[role=menu]').exists()).toBe(false);
  await view.get('[aria-label="文件"]').trigger('click');
  window.dispatchEvent(new Event('blur'));
  await flushPromises();
  expect(view.find('[role=menu]').exists()).toBe(false);
  const removeWindow = vi.spyOn(window, 'removeEventListener');
  const removeDocument = vi.spyOn(document, 'removeEventListener');
  view.unmount();
  wrapper = undefined;
  expect(removeWindow).toHaveBeenCalledWith('keydown', expect.any(Function));
  expect(removeWindow).toHaveBeenCalledWith('blur', expect.any(Function));
  expect(removeDocument).toHaveBeenCalledWith('pointerdown', expect.any(Function));
  expect(removeDocument).toHaveBeenCalledWith('focusin', expect.any(Function));
});

it('菜单内的命令快捷键复用菜单动作，不将普通 V 或重复按键冒泡到画布', async () => {
  const view = setup();
  await view.get('[aria-label="文件"]').trigger('click');
  await view.get('[aria-label="打开 PSD"]').trigger('keydown', { key: 'v' });
  await view
    .get('[aria-label="打开 PSD"]')
    .trigger('keydown', { key: 'o', ctrlKey: true, repeat: true });
  expect(view.emitted('command')).toBeUndefined();
  await view.get('[aria-label="打开 PSD"]').trigger('keydown', { key: 'o', ctrlKey: true });
  expect(view.emitted('command')).toEqual([['open']]);
  expect(view.find('[role=menu]').exists()).toBe(false);
});

it('全部命令禁用时仍保留菜单焦点，允许 Esc 收起和方向键切换', async () => {
  const view = setup();
  await view.setProps({
    menus: menus.map((menu) =>
      menu.id === 'view'
        ? { ...menu, items: menu.items.map((item) => ({ ...item, disabled: true })) }
        : menu,
    ),
  });
  await view.get('[aria-label="文件"]').trigger('click');
  await view.get('[aria-label="打开 PSD"]').trigger('keydown', { key: 'ArrowRight' });
  expect(document.activeElement).toBe(view.get('[aria-label="视图"]').element);
  await view.get('[aria-label="视图"]').trigger('keydown', { key: 'Escape' });
  expect(view.find('[role=menu]').exists()).toBe(false);
  expect(view.emitted('command')).toBeUndefined();
});
