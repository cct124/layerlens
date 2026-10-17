// 失败采样也保留完整身份与错误证据；不把缺测或部分进程总和写成有效内存读数。
import assert from 'node:assert/strict';

export function recordMemorySample(samples, metrics, rootProcessId, detail) {
  const total = metrics.processes.reduce((sum, p) => sum + p.privateBytes, 0);
  const valid =
    metrics.rootProcessId === rootProcessId &&
    metrics.errors.length === 0 &&
    metrics.processes.some((p) => p.role === 'application' && p.processId === rootProcessId) &&
    metrics.processes.some((p) => p.role === 'webview') &&
    metrics.processes.every((p) =>
      [p.privateBytes, p.workingSetBytes].every((n) => Number.isSafeInteger(n) && n > 0),
    ) &&
    Number.isSafeInteger(total);
  samples.push({ ...detail, ...metrics, valid, privateBytes: valid ? total : null });
  assert(valid, '内存采样无效，完整证据已保留：' + JSON.stringify(metrics.errors));
  return total;
}
