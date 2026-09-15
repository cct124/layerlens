import { onMounted, ref } from 'vue';
import { getAppInfo, isDesktopAvailable } from '../../shared/api/desktop';
import type { AppInfo } from '../../shared/api/generated';

type AppStatus =
  | { phase: 'checking' }
  | { phase: 'browser' }
  | { phase: 'ready'; info: AppInfo }
  | { phase: 'error'; message: string };

/** 管理启动页的桌面连接检查，不保存或模拟文档业务状态。 */
export function useAppStatus() {
  const status = ref<AppStatus>({ phase: 'checking' });
  let pending = false;

  async function checkStatus(): Promise<void> {
    if (pending) return;
    if (!isDesktopAvailable()) {
      status.value = { phase: 'browser' };
      return;
    }

    pending = true;
    status.value = { phase: 'checking' };
    try {
      status.value = { phase: 'ready', info: await getAppInfo() };
    } catch (error: unknown) {
      status.value = {
        phase: 'error',
        message: error instanceof Error ? error.message : '应用状态检查失败，请重试。',
      };
    } finally {
      pending = false;
    }
  }

  onMounted(checkStatus);
  return { status, checkStatus };
}
