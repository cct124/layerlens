import { nextTick, onMounted, onScopeDispose, watch } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import type { UiObservation } from '../../shared/api/devGenerated';

type Projection = Omit<
  UiObservation,
  'instanceId' | 'frontendId' | 'documentEpoch' | 'viewRevision' | 'menu' | 'focus' | 'input'
>;
declare global {
  interface Window {
    __LAYERLENS_DEV_BOOT__?: unknown;
    __LAYERLENS_DEV_VIEW__?: { read(): UiObservation | null; readonly error: string | null };
  }
}

/** 仅显式测试实例安装。前端实际应用状态后上报；只读桥不提供任何状态修改方法。 */
export function developmentInstance(): string | null {
  const boot = window.__LAYERLENS_DEV_BOOT__;
  return boot &&
    typeof boot === 'object' &&
    'instanceId' in boot &&
    typeof boot.instanceId === 'string'
    ? boot.instanceId
    : null;
}

export function useDevObservation(project: () => Projection) {
  let stop: (() => void) | undefined;
  onMounted(() => {
    const bootstrapId = developmentInstance();
    if (!bootstrapId) return;
    const instanceId = bootstrapId;
    const frontendId = crypto.randomUUID();
    // 同一页面的组件重挂载也建立新代次，不沿用旧挂载的报告身份。
    const documentEpoch = Math.trunc(
      (performance.timeOrigin + performance.now()) * 1000,
    ).toString();
    let disposed = false,
      scheduled = false,
      sending = false;
    let revision = 0n,
      pointers = 0n,
      keys = 0n;
    let lastKey: string | null = null,
      transportError: string | null = null;
    let previous = '',
      latest: UiObservation | null = null,
      pending: UiObservation | null = null;
    async function pump() {
      if (disposed || sending || !pending) return;
      sending = true;
      const observation = pending;
      pending = null;
      try {
        await invoke('dev_view_report', { observation });
        transportError = null;
      } catch (cause: unknown) {
        if (!disposed) {
          const message = cause instanceof Error ? cause.message : JSON.stringify(cause);
          if (transportError !== message) console.warn('开发观测上报失败', message);
          transportError = message;
        }
      } finally {
        sending = false;
        if (!disposed) void pump();
      }
    }
    function publish() {
      if (disposed) return;
      const selectedMenu = document.querySelector<HTMLElement>('.menu-trigger[aria-expanded=true]');
      const focused = document.activeElement;
      const value = {
        ...project(),
        menu: selectedMenu?.id.replace('menu-trigger-', '') ?? null,
        focus:
          focused instanceof HTMLElement
            ? (focused.dataset.devTarget ?? (focused.id || focused.tagName)).slice(0, 128)
            : null,
        input: { pointerEvents: pointers.toString(), keyEvents: keys.toString(), lastKey },
      };
      const serialized = JSON.stringify(value);
      if (serialized === previous) return;
      previous = serialized;
      latest = {
        ...value,
        instanceId,
        frontendId,
        documentEpoch,
        viewRevision: (++revision).toString(),
      };
      pending = latest;
      void pump();
    }
    function schedule() {
      if (scheduled || disposed) return;
      scheduled = true;
      // 捕获阶段先排到微任务，再取得 nextTick：让同一输入的后续事件处理器先排入 Vue 更新。
      void Promise.resolve()
        .then(() => nextTick())
        .then(() => {
          scheduled = false;
          publish();
        });
    }
    function pointer(event: PointerEvent) {
      if (event.isTrusted) pointers++;
      schedule();
    }
    function key(event: KeyboardEvent) {
      if (event.isTrusted) {
        keys++;
        // 不记录任务名、原文或一般键入字符，仅保留测试所需控制键。
        if (
          [
            'Escape',
            'Alt',
            'Control',
            'Shift',
            'Tab',
            'Enter',
            'ArrowLeft',
            'ArrowRight',
            'ArrowUp',
            'ArrowDown',
          ].includes(event.key)
        )
          lastKey = event.key;
      }
      schedule();
    }
    const bridge = Object.freeze({
      read: () => (latest ? structuredClone(latest) : null),
      get error() {
        return transportError;
      },
    });
    Object.defineProperty(window, '__LAYERLENS_DEV_VIEW__', { value: bridge, configurable: true });
    const unwatch = watch(project, schedule, { deep: true, flush: 'post' });
    const observer = new MutationObserver(schedule);
    observer.observe(document.body, {
      subtree: true,
      childList: true,
      attributes: true,
      attributeFilter: ['open', 'aria-expanded', 'aria-selected'],
    });
    document.addEventListener('pointerdown', pointer, true);
    document.addEventListener('pointerup', pointer, true);
    document.addEventListener('keydown', key, true);
    document.addEventListener('keyup', key, true);
    document.addEventListener('focusin', schedule);
    document.addEventListener('load', schedule, true);
    document.addEventListener('error', schedule, true);
    window.addEventListener('resize', schedule);
    stop = () => {
      disposed = true;
      pending = null;
      unwatch();
      observer.disconnect();
      document.removeEventListener('pointerdown', pointer, true);
      document.removeEventListener('pointerup', pointer, true);
      document.removeEventListener('keydown', key, true);
      document.removeEventListener('keyup', key, true);
      document.removeEventListener('focusin', schedule);
      document.removeEventListener('load', schedule, true);
      document.removeEventListener('error', schedule, true);
      window.removeEventListener('resize', schedule);
      if (window.__LAYERLENS_DEV_VIEW__ === bridge) delete window.__LAYERLENS_DEV_VIEW__;
    };
    schedule();
  });
  onScopeDispose(() => stop?.());
}
