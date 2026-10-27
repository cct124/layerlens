import { afterEach, expect, it, vi } from 'vitest';
import { observeCleanupLayout } from './devCleanupLayout';
afterEach(() => vi.restoreAllMocks());
function rect(element: Element, x: number, y: number, width: number, height: number) {
  vi.spyOn(element, 'getBoundingClientRect').mockReturnValue({
    x,
    y,
    width,
    height,
    left: x,
    top: y,
    right: x + width,
    bottom: y + height,
    toJSON: () => ({}),
  });
  Object.defineProperty(element, 'clientWidth', { configurable: true, value: width });
  Object.defineProperty(element, 'clientHeight', { configurable: true, value: height });
}
function fixture() {
  const dialog = document.createElement('dialog');
  dialog.innerHTML =
    '<ul class="cleanup-tasks"><li>first</li><li>last</li></ul><footer><button>cancel</button><button>confirm</button></footer>';
  const list = dialog.querySelector('ul')!;
  rect(dialog, 20, 20, 500, 500);
  rect(list, 40, 200, 450, 100);
  rect(list.children[0]!, 40, 200, 400, 20);
  rect(list.children[1]!, 40, 500, 400, 20);
  for (const button of dialog.querySelectorAll('button')) rect(button, 100, 450, 100, 30);
  return { dialog, list };
}
it('末项在 DOM 中但被列表滚动裁切时不可见，真实末端几何到达后才可见', () => {
  const { dialog, list } = fixture();
  expect(observeCleanupLayout(dialog)).toMatchObject({
    firstTaskVisible: true,
    lastTaskVisible: false,
    controlsVisible: true,
    dialogInsideViewport: true,
  });
  rect(list.children[0]!, 40, -80, 400, 20);
  rect(list.children[1]!, 40, 280, 400, 20);
  list.scrollTop = 280;
  expect(observeCleanupLayout(dialog)).toMatchObject({
    firstTaskVisible: false,
    lastTaskVisible: true,
    scrolledToStart: false,
  });
});
it('列表内可见不等于模态／屏幕内可见，按钮必须完整可达', () => {
  const { dialog, list } = fixture();
  rect(dialog, 20, 20, 500, 250);
  rect(list.children[1]!, 40, 280, 400, 20);
  expect(observeCleanupLayout(dialog)).toMatchObject({
    lastTaskVisible: false,
    controlsVisible: false,
  });
  rect(dialog, 20, -30, 500, 500);
  expect(observeCleanupLayout(dialog).dialogInsideViewport).toBe(false);
  expect(observeCleanupLayout(null).controlsVisible).toBe(false);
});
