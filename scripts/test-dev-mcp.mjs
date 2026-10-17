// 在独立的真实桌面进程中验证 MCP 业务路径；不把此结果标记为 WebView 输入或视觉验收。
import assert from 'node:assert/strict';
import { execFileSync, spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import { createReadStream } from 'node:fs';
import { mkdir, mkdtemp, readFile, readdir, stat, watch, writeFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';
import { redact } from './dev-mcp-artifacts.mjs';
import { checkSelectionObservation } from './dev-mcp-runner.mjs';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const { values: options } = parseArgs({
  options: {
    app: { type: 'string' },
    'expect-disabled': { type: 'boolean', default: false },
    'output-root': { type: 'string' },
  },
});
if (process.platform !== 'win32') throw new Error('当前真实桌面验收仅支持 Windows。');
const executable = path.resolve(root, options.app ?? 'target/debug/layerlens.exe');
assert((await stat(executable)).isFile(), '请先运行 npm run build:dev-mcp');
const sampleRoot = path.join(root, 'crates/layerlens-core/tests/fixtures/psd');
const base = path.resolve(root, options['output-root'] ?? '.local/dev-mcp');
await mkdir(base, { recursive: true });
const run = await mkdtemp(path.join(base, 'desktop-'));
const runs = path.join(run, 'instances');
await mkdir(runs);
const config = path.join(run, 'config.json');
await writeFile(config, JSON.stringify({ sampleRoot, runRoot: runs, port: 0, visible: false }), {
  flag: 'wx',
});
async function fileHash(file) {
  const hash = createHash('sha256');
  for await (const chunk of createReadStream(file)) hash.update(chunk);
  return hash.digest('hex');
}
const fingerprint = (file) => fileHash(path.join(sampleRoot, file));
const sourceFiles = ['bitmap-raw.psd', 'bitmap-rle.psd'];
const hashes = await Promise.all(sourceFiles.map(fingerprint));
const report = {
  evidence: options['expect-disabled']
    ? 'desktop-process-feature-disabled'
    : 'desktop-process-mcp-business',
  app: executable,
  appSha256: await fileHash(executable),
  cargoLockSha256: await fileHash(path.join(root, 'Cargo.lock')),
  commit: execFileSync('git', ['rev-parse', 'HEAD'], {
    cwd: root,
    encoding: 'utf8',
    windowsHide: true,
  }).trim(),
  dirty:
    execFileSync('git', ['status', '--porcelain'], {
      cwd: root,
      encoding: 'utf8',
      windowsHide: true,
    }).trim().length > 0,
  system: {
    platform: process.platform,
    release: os.release(),
    arch: process.arch,
    node: process.version,
  },
  startedAt: new Date().toISOString(),
  actions: [],
  sourceFiles,
  sourceSha256: hashes,
  checks: [],
  passed: false,
};
const started = performance.now();
const child = spawn(executable, ['--dev-mcp-config', config], {
  cwd: root,
  windowsHide: true,
  stdio: ['ignore', 'pipe', 'pipe'],
});
let diagnostic = '';
const append = (chunk) => {
  diagnostic = (diagnostic + chunk.toString()).slice(-32 * 1024);
};
child.stdout.on('data', append);
child.stderr.on('data', append);
let exited = false;
const stopped = new AbortController();
const exit = new Promise((resolve, reject) => {
  child.once('error', (error) => {
    exited = true;
    stopped.abort();
    reject(error);
  });
  child.once('exit', (code, signal) => {
    exited = true;
    stopped.abort();
    resolve({ code, signal });
  });
});
// 错误在主流程通过 exit 读取；立即安装处理器，避免启动失败形成未处理拒绝。
exit.catch(() => undefined);
async function waitForExit(timeoutMs) {
  let timeout;
  try {
    return await Promise.race([
      exit,
      new Promise((_, reject) => {
        timeout = setTimeout(() => reject(new Error('桌面进程未在期限内退出')), timeoutMs);
      }),
    ]);
  } finally {
    clearTimeout(timeout);
  }
}

async function connectionInfo() {
  const controller = new AbortController();
  const signal = AbortSignal.any([controller.signal, stopped.signal, AbortSignal.timeout(30_000)]);
  const events = watch(runs, { recursive: true, signal });
  let event = events.next();
  event.catch(() => undefined);
  try {
    for (;;) {
      for (const entry of await readdir(runs, { withFileTypes: true })) {
        if (!entry.isDirectory()) continue;
        const directory = path.join(runs, entry.name);
        const files = await readdir(directory);
        if (!files.includes('ready')) continue;
        const connection = JSON.parse(
          await readFile(path.join(directory, 'connection.json'), 'utf8'),
        );
        assert.equal(connection.processId, child.pid);
        assert.equal(connection.formatVersion, 1);
        assert.match(connection.instanceId, /^[0-9a-f-]{36}$/);
        assert.match(connection.token, /^[0-9a-f]{64}$/);
        const endpoint = new URL(connection.endpoint);
        assert.equal(endpoint.protocol, 'http:');
        assert.equal(endpoint.hostname, '127.0.0.1');
        assert.equal(endpoint.pathname, '/mcp');
        assert.equal(endpoint.username + endpoint.password, '');
        return { ...connection, directory };
      }
      await event;
      event = events.next();
      event.catch(() => undefined);
    }
  } finally {
    controller.abort();
    await events.return().catch((error) => {
      if (error.name !== 'AbortError') throw error;
    });
  }
}

let connection;
let info;
let requestId = 0;
let mutationId = 0;
async function rpc(method, params, notification = false) {
  const id = notification ? undefined : ++requestId;
  const action = {
    id,
    method,
    tool: params?.name,
    clientRequestId: params?.arguments?.clientRequestId,
    elapsedMs: Math.round(performance.now() - started),
  };
  report.actions.push(action);
  const response = await fetch(connection.endpoint, {
    method: 'POST',
    headers: {
      Authorization: 'Bearer ' + connection.token,
      'Content-Type': 'application/json',
      Accept: 'application/json, text/event-stream',
      'MCP-Protocol-Version': '2025-11-25',
    },
    body: JSON.stringify({ jsonrpc: '2.0', id, method, params }),
    signal: AbortSignal.timeout(15_000),
  });
  action.httpStatus = response.status;
  assert(response.ok, 'MCP HTTP 状态：' + response.status);
  if (notification) return;
  const reply = await response.json();
  action.errorCode = reply.error?.code ?? reply.result?.structuredContent?.data?.error?.code;
  action.isError = reply.result?.isError;
  assert.equal(reply.id, id);
  assert.equal(reply.error, undefined, JSON.stringify(reply.error));
  return reply.result;
}
function argumentsFor(request, mutation = false) {
  return {
    instanceId: info.instanceId,
    sessionId: info.sessionId,
    clientRequestId: mutation ? 'desktop-' + ++mutationId : null,
    request,
  };
}
async function tool(name, args, error = false) {
  const result = await rpc('tools/call', { name, arguments: args });
  assert.equal(result.isError, error);
  assert.deepEqual(JSON.parse(result.content[0].text), result.structuredContent);
  const unavailable = checkSelectionObservation(
    result.structuredContent.userSelection,
    info?.sessionId ?? result.structuredContent.data.value.sessionId,
  );
  if (unavailable) {
    report.unavailableObservations ??= [];
    report.unavailableObservations.push({
      tool: name,
      clientRequestId: args.clientRequestId,
      ...unavailable,
    });
  }
  return result.structuredContent;
}
const workspace = (action, mutation = true) =>
  tool('dev_workspace', argumentsFor({ protocolVersion: 1, action }, mutation));
const selection = (operation, mutation = true) =>
  tool(
    'dev_selection',
    argumentsFor({ protocolVersion: 1, sessionId: info.sessionId, operation }, mutation),
  );
const cleanup = (operation) =>
  tool(
    'dev_cleanup',
    argumentsFor({ protocolVersion: 1, sessionId: info.sessionId, operation }, true),
  );
async function waitFor(condition) {
  const result = await tool('dev_wait', argumentsFor({ condition, timeoutMs: 10_000 }));
  assert.equal(result.data.value.satisfied, true);
  return result.data.value.lastState;
}

async function exerciseInstance() {
  connection = await connectionInfo();
  const initialization = await rpc('initialize', {
    protocolVersion: '2025-11-25',
    capabilities: {},
    clientInfo: { name: 'layerlens-desktop-regression', version: '1' },
  });
  assert.equal(initialization.protocolVersion, '2025-11-25');
  await rpc('notifications/initialized', {}, true);
  const listed = await rpc('tools/list', {});
  assert.equal(listed.tools.length, 11);
  assert(listed.tools.every((entry) => entry.inputSchema && entry.outputSchema));
  info = (await tool('dev_instance', {})).data.value;
  report.instanceId = info.instanceId;
  report.sessionId = info.sessionId;
  report.appVersion = info.appVersion;
  report.limits = info.limits;
  assert.equal(info.instanceId, connection.instanceId);
  const metrics = (await tool('dev_process_metrics', argumentsFor({}))).data.value;
  assert.equal(metrics.rootProcessId, connection.processId);
  assert.equal(metrics.errors.length, 0, JSON.stringify(metrics));
  assert(metrics.processes.some((p) => p.processId === connection.processId && p.privateBytes > 0));
  report.processMetrics = metrics;
  report.checks.push('通用开发 MCP 采集当前隔离实例的实际进程内存');
  report.checks.push('initialize、工具 Schema 与实例身份');
  for (const [index, name] of sourceFiles.entries()) {
    await workspace({ kind: 'open', path: name });
    await waitFor({ kind: 'documentCount', count: index + 1 });
  }
  let state = (await workspace({ kind: 'snapshot' }, false)).data.value.snapshot;
  const a = state.documents.find((document) => document.name === sourceFiles[0]);
  const b = state.documents.find((document) => document.name === sourceFiles[1]);
  assert(a && b);
  await workspace({ kind: 'activate', documentId: a.id });
  state = await waitFor({ kind: 'previewReady', documentId: a.id, revision: a.revision });
  // 核心就绪时前端可能正在复制同一 PNG；等实际 decode/绘制准备，保持单次复制预算和 BUSY 语义。
  await waitFor({
    kind: 'frontendImageReady',
    documentId: a.id,
    revision: a.revision,
    atLeast: state.sequence,
  });
  const preview = await rpc('tools/call', {
    name: 'dev_preview',
    arguments: argumentsFor({ protocolVersion: 1, documentId: a.id, revision: a.revision }),
  });
  assert.equal(preview.isError, false);
  const png = Buffer.from(
    preview.content.find((content) => content.type === 'image').data,
    'base64',
  );
  assert.deepEqual([...png.subarray(0, 8)], [137, 80, 78, 71, 13, 10, 26, 10]);
  assert.equal(png.readUInt32BE(16), 4);
  assert.equal(png.readUInt32BE(20), 3);
  const scope = await selection({
    kind: 'region',
    documentId: a.id,
    documentRevision: a.revision,
    expectedRevision: state.selection.selectionRevision,
    bounds: { x: 0, y: 0, width: 4, height: 3 },
  });
  assert.equal(scope.userSelection.content.layers[0].details.layer.name, 'negative-origin');
  assert.equal(scope.userSelection.content.snapshotId, scope.data.value.snapshotId);
  const task = (
    await selection({
      kind: 'createTask',
      snapshotId: scope.data.value.snapshotId,
      requestId: 'desktop-fixed-a',
      name: 'MCP 桌面验收 A',
    })
  ).data.value.task;
  await selection({
    kind: 'region',
    documentId: a.id,
    documentRevision: a.revision,
    expectedRevision: scope.userSelection.selectionRevision,
    bounds: { x: 0, y: 1, width: 2, height: 1 },
  });
  await workspace({ kind: 'activate', documentId: b.id });
  await workspace({ kind: 'close', documentId: a.id });
  const content = {
    kind: 'content',
    target: { kind: 'task', taskId: task.id },
    cursor: null,
    limit: 32,
  };
  const retained = await selection(content, false);
  assert.equal(retained.data.value.page.snapshotId, task.scope.snapshotId);
  assert.equal(retained.data.value.page.documentRevision, a.revision);
  assert.equal(retained.userSelection.documentId, b.id);
  report.checks.push('打开、预览、区域内容与关闭后固定任务读取');
  await workspace({ kind: 'open', path: sourceFiles[0] });
  state = await waitFor({ kind: 'documentCount', count: 2 });
  const fresh = state.documents.find((document) => document.name === sourceFiles[0]);
  assert.notEqual(fresh.id, a.id);
  const plan = (await cleanup({ kind: 'prepare', target: { kind: 'task', taskId: task.id } })).data
    .value.plan;
  await cleanup({ kind: 'cancel', planId: plan.planId });
  await selection(content, false);
  const confirmation = (
    await cleanup({ kind: 'prepare', target: { kind: 'task', taskId: task.id } })
  ).data.value.plan;
  const confirm = argumentsFor(
    {
      protocolVersion: 1,
      sessionId: info.sessionId,
      operation: { kind: 'confirm', planId: confirmation.planId },
    },
    true,
  );
  const receipt = await tool('dev_cleanup', confirm);
  assert.deepEqual(receipt.data.value.receipt.invalidatedTaskIds, [task.id]);
  assert.deepEqual((await tool('dev_cleanup', confirm)).data, receipt.data);
  const invalidated = await tool(
    'dev_selection',
    argumentsFor({ protocolVersion: 1, sessionId: info.sessionId, operation: content }),
    true,
  );
  assert.equal(invalidated.data.error.code, 'TASK_INVALIDATED');
  state = (await workspace({ kind: 'snapshot' }, false)).data.value.snapshot;
  assert.deepEqual(state.documents.map((document) => document.id).sort(), [b.id, fresh.id].sort());
  report.checks.push('清理取消、确认幂等、任务失效与同路径重开隔离');
  for (const document of state.documents)
    await workspace({ kind: 'close', documentId: document.id });
  state = await waitFor({ kind: 'idle' });
  report.finalState = state;
  assert.equal(state.resources.sourceBytes, '0');
  assert.deepEqual(await Promise.all(sourceFiles.map(fingerprint)), hashes);
  report.checks.push('源文件不变与源数据账本释放');
  await tool('dev_shutdown', argumentsFor({}));
  const outcome = await waitForExit(10_000);
  assert.equal(outcome.code, 0, diagnostic);
  assert((await readdir(connection.directory)).includes('stopped'));
  assert(!(await readdir(connection.directory)).includes('connection.json'));
  report.checks.push('MCP 正常退出、监听与凭据文件回收');
}

try {
  if (options['expect-disabled']) {
    const outcome = await waitForExit(10_000);
    assert.equal(outcome.code, 1, '普通构建必须拒绝开发参数并退出');
    assert.match(diagnostic, /当前构建未启用 dev-mcp 特性/);
    assert.deepEqual(await readdir(runs), [], '普通构建不能创建测试实例');
    report.checks.push('普通构建拒绝开发参数，未创建监听与实例');
  } else {
    await exerciseInstance();
  }
  report.passed = true;
} catch (error) {
  report.error = error.stack ?? String(error);
  process.exitCode = 1;
} finally {
  if (!exited && connection && info) {
    try {
      await tool('dev_shutdown', argumentsFor({}));
    } catch (error) {
      report.shutdownError = String(error);
    }
  }
  if (!exited) {
    const force = setTimeout(() => {
      if (!exited) {
        report.forcedExit = true;
        child.kill();
      }
    }, 5000);
    try {
      await waitForExit(10_000);
    } catch (error) {
      report.processError = String(error);
    } finally {
      clearTimeout(force);
    }
  }
  if (diagnostic)
    await writeFile(path.join(run, 'process.log'), redact(diagnostic, [connection?.token]), {
      flag: 'wx',
    });
  report.durationMs = Math.round(performance.now() - started);
  await writeFile(
    path.join(run, 'report.json'),
    redact(JSON.stringify(report, null, 2), [connection?.token]),
    { flag: 'wx' },
  );
}
console.log(
  redact(
    JSON.stringify(
      {
        passed: report.passed,
        checks: report.checks,
        report: path.join(run, 'report.json'),
        error: report.error,
      },
      null,
      2,
    ),
    [connection?.token],
  ),
);
