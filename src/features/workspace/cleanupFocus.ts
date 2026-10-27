/** 显式保持清理模态的 Tab 循环；部分 WebView 会在末控件后把焦点交给宿主。 */
export function cycleCleanupFocus(event: KeyboardEvent, dialog: HTMLDialogElement | null) {
  if (
    !dialog?.open ||
    event.key !== 'Tab' ||
    event.defaultPrevented ||
    event.altKey ||
    event.ctrlKey ||
    event.metaKey ||
    event.isComposing
  )
    return;
  const controls = [
    ...dialog.querySelectorAll<HTMLElement>('button:not([disabled]), summary, [tabindex]'),
  ].filter(
    (element) =>
      (element.tagName === 'SUMMARY' || element.tabIndex >= 0) &&
      element.getClientRects().length > 0,
  );
  const first = controls[0],
    last = controls.at(-1);
  if (!first || !last) {
    event.preventDefault();
    dialog.focus();
    return;
  }
  const active = document.activeElement;
  if (event.shiftKey && (active === first || !controls.some((c) => c === active))) {
    event.preventDefault();
    last.focus();
  } else if (!event.shiftKey && active === last) {
    event.preventDefault();
    first.focus();
  }
}
