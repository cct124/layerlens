import { afterEach, expect, it, vi } from 'vitest';
import { cycleCleanupFocus } from './cleanupFocus';
function rectangles(visible = true): DOMRectList {
  const values = visible ? [new DOMRect(0, 0, 20, 20)] : [];
  return Object.assign(values, { item: (index: number) => values[index] ?? null });
}
afterEach(() => {
  document.body.replaceChildren();
  vi.restoreAllMocks();
});
it('确认按钮末端 Tab 回到摘要，首项 Shift+Tab 回到确认，跳过禁用和不可见控件', () => {
  const dialog = document.createElement('dialog');
  dialog.open = true;
  dialog.innerHTML =
    '<h2 tabindex="-1">title</h2><details open><summary>tasks</summary><ul tabindex="0"></ul></details><button>cancel</button><button disabled>busy</button><button hidden>hidden</button><button>confirm</button>';
  document.body.append(dialog);
  for (const el of dialog.querySelectorAll<HTMLElement>('summary,ul,button')) {
    vi.spyOn(el, 'getClientRects').mockReturnValue(rectangles(!el.hidden));
  }
  const first = dialog.querySelector<HTMLElement>('summary')!,
    last = dialog.querySelectorAll<HTMLButtonElement>('button')[3]!;
  last.focus();
  const forward = new KeyboardEvent('keydown', { key: 'Tab', cancelable: true });
  cycleCleanupFocus(forward, dialog);
  expect(forward.defaultPrevented).toBe(true);
  expect(document.activeElement).toBe(first);
  const back = new KeyboardEvent('keydown', { key: 'Tab', shiftKey: true, cancelable: true });
  cycleCleanupFocus(back, dialog);
  expect(back.defaultPrevented).toBe(true);
  expect(document.activeElement).toBe(last);
  dialog.querySelector('h2')!.focus();
  cycleCleanupFocus(
    new KeyboardEvent('keydown', { key: 'Tab', shiftKey: true, cancelable: true }),
    dialog,
  );
  expect(document.activeElement).toBe(last);
});
it('中间控件和 Esc 使用原行为，已关闭模态不捕获按键', () => {
  const dialog = document.createElement('dialog');
  dialog.open = true;
  dialog.innerHTML = '<button>first</button><button>middle</button><button>last</button>';
  document.body.append(dialog);
  for (const el of dialog.querySelectorAll('button'))
    vi.spyOn(el, 'getClientRects').mockReturnValue(rectangles());
  dialog.querySelectorAll('button')[1]!.focus();
  for (const key of ['Tab', 'Escape']) {
    const e = new KeyboardEvent('keydown', { key, cancelable: true });
    cycleCleanupFocus(e, dialog);
    expect(e.defaultPrevented).toBe(false);
  }
  dialog.open = false;
  const e = new KeyboardEvent('keydown', { key: 'Tab', cancelable: true, shiftKey: true });
  cycleCleanupFocus(e, dialog);
  expect(e.defaultPrevented).toBe(false);
});
