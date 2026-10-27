import type { UiCleanupLayout } from '../../shared/api/devGenerated';

/** 只读实际布局；同时考虑视口、模态滚动裁切及列表自身滚动裁切。 */
export function observeCleanupLayout(dialog: Element | null): UiCleanupLayout {
  const empty: UiCleanupLayout = {
    dialogInsideViewport: false,
    firstTaskVisible: false,
    lastTaskVisible: false,
    controlsVisible: false,
    scrolledToStart: false,
  };
  if (!dialog) return empty;
  const viewport = { x: 0, y: 0, width: window.innerWidth, height: window.innerHeight };
  const contains = (inner: DOMRect | typeof viewport, outer: DOMRect | typeof viewport) =>
    inner.width > 0 &&
    inner.height > 0 &&
    inner.x >= outer.x - 1 &&
    inner.y >= outer.y - 1 &&
    inner.x + inner.width <= outer.x + outer.width + 1 &&
    inner.y + inner.height <= outer.y + outer.height + 1;
  const clip = (element: Element) => {
    const r = element.getBoundingClientRect();
    return {
      x: r.x + element.clientLeft,
      y: r.y + element.clientTop,
      width: element.clientWidth,
      height: element.clientHeight,
    };
  };
  const dialogClip = clip(dialog);
  const visible = (element: Element | null, within?: Element) => {
    if (!element) return false;
    const r = element.getBoundingClientRect();
    return (
      contains(r, viewport) && contains(r, dialogClip) && (!within || contains(r, clip(within)))
    );
  };
  const list = dialog.querySelector('.cleanup-tasks');
  const controls = dialog.querySelectorAll('footer button');
  return {
    dialogInsideViewport: contains(dialog.getBoundingClientRect(), viewport),
    firstTaskVisible: !!list && visible(list.firstElementChild, list),
    lastTaskVisible: !!list && visible(list.lastElementChild, list),
    controlsVisible: controls.length > 0 && [...controls].every((c) => visible(c)),
    scrolledToStart: !!list && list.scrollTop <= 1,
  };
}
