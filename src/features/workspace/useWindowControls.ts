import { computed, onMounted, onScopeDispose, ref } from 'vue';
import { getCurrentWindow } from '@tauri-apps/api/window';
import type { UnlistenFn } from '@tauri-apps/api/event';

type WindowCommand = 'minimize' | 'toggleMaximize' | 'close';
const commandNames: Record<WindowCommand, string> = {
  minimize: '最小化窗口',
  toggleMaximize: '最大化 / 还原窗口',
  close: '关闭窗口',
};
const message = (cause: unknown) => (cause instanceof Error ? cause.message : String(cause));

/** 仅管理桌面窗口，不复制工作区状态；关闭请求交给 Tauri 既有退出清理流程。 */
export function useWindowControls(desktop: boolean) {
  const appWindow = desktop ? getCurrentWindow() : null;
  const maximized = ref<boolean | null>(null);
  const pending = ref<WindowCommand | null>(null);
  const actionError = ref<string | null>(null);
  const stateError = ref<string | null>(null);
  let disposed = false;
  let unsubscribe: UnlistenFn | undefined;
  let refreshing = false;
  let refreshAgain = false;
  let dragging = false;

  async function refresh() {
    if (!appWindow || disposed) return;
    refreshAgain = true;
    if (refreshing) return;
    refreshing = true;
    try {
      // resize 可密集到达，最多一个查询在途；旧结果不覆盖更新事件要求的重读。
      while (refreshAgain && !disposed) {
        refreshAgain = false;
        try {
          const value = await appWindow.isMaximized();
          if (!disposed && !refreshAgain) {
            maximized.value = value;
            stateError.value = null;
          }
        } catch (cause: unknown) {
          if (!disposed) stateError.value = `读取窗口状态失败：${message(cause)}`;
        }
      }
    } finally {
      refreshing = false;
    }
  }

  async function run(command: WindowCommand) {
    if (!appWindow || disposed || pending.value) return;
    pending.value = command;
    actionError.value = null;
    try {
      // close() 发出正常关闭请求；不使用 destroy() 绕过生命周期。
      await appWindow[command]();
      if (command === 'toggleMaximize') await refresh();
    } catch (cause: unknown) {
      if (!disposed) actionError.value = `${commandNames[command]}失败：${message(cause)}`;
    } finally {
      if (!disposed) pending.value = null;
    }
  }

  async function startDragging() {
    if (!appWindow || disposed || pending.value || dragging) return;
    dragging = true;
    actionError.value = null;
    try {
      await appWindow.startDragging();
    } catch (cause: unknown) {
      if (!disposed) actionError.value = `拖动窗口失败：${message(cause)}`;
    } finally {
      dragging = false;
    }
  }

  onMounted(async () => {
    if (!appWindow) return;
    window.addEventListener('focus', refresh);
    try {
      const stop = await appWindow.onResized(() => void refresh());
      if (disposed) stop();
      else unsubscribe = stop;
    } catch (cause: unknown) {
      if (!disposed) actionError.value = `订阅窗口状态失败：${message(cause)}`;
    }
    await refresh();
  });
  onScopeDispose(() => {
    disposed = true;
    window.removeEventListener('focus', refresh);
    unsubscribe?.();
  });

  return {
    maximized,
    pending,
    error: computed(() => actionError.value ?? stateError.value),
    dismissError() {
      actionError.value = null;
      stateError.value = null;
    },
    run,
    startDragging,
  };
}
