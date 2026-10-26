// 第二批真实 WebView 回归：应用操作经 MCP → W3C WebDriver → 前端事件 → IPC。
// 驱动会话启动、页面刷新与退出监测由本运行器管理。
import assert from 'node:assert/strict';
import { execFileSync, spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import { createReadStream } from 'node:fs';
import { mkdir, mkdtemp, readdir, readFile, writeFile, watch, open } from 'node:fs/promises';
import path from 'node:path';
import os from 'node:os';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';
import { redact } from './dev-mcp-artifacts.mjs';
import { errorDiagnostic, observeProcess } from './dev-mcp-process-monitor.mjs';
import { exerciseOpenCleanup, exerciseRegionControls } from './dev-mcp-m2-webview.mjs';
import { exerciseCanvasInput } from './dev-mcp-input-webview.mjs';
import { exerciseCleanupBoundaries } from './dev-mcp-cleanup-webview.mjs';
import { exerciseWindows } from './dev-mcp-window-webview.mjs';
import { exerciseRenderLab } from './dev-mcp-render-webview.mjs';
import { exerciseRenderBenchmark } from './dev-mcp-render-benchmark.mjs';
import { exerciseRenderAcceptance } from './dev-mcp-render-acceptance.mjs';
import { exerciseRenderProject } from './dev-mcp-render-project.mjs';
import { captureTiled } from './dev-mcp-capture.mjs';
import { checkDevPython, runDevPython } from './dev-python.mjs';
import { createDisplayGuard } from './dev-mcp-render-environment.mjs';
import { exerciseDisplayEnvironment } from './dev-mcp-display-webview.mjs';
import { preflightOptions, prepareTestWindow } from './dev-mcp-preflight.mjs';
import {
  exerciseWorkspaceCanvas,
  recordWorkspaceEnvironment,
} from './dev-mcp-workspace-canvas.mjs';

const { values: options } = parseArgs({
  options: {
    driver: { type: 'string' },
    app: { type: 'string' },
    visible: { type: 'boolean', default: false },
    'output-root': { type: 'string' },
    scenario: { type: 'string', default: 'baseline' },
    'canvas-memory-focus': { type: 'string', default: 'main' },
    'window-size': { type: 'string' },
    'expect-dpr': { type: 'string' },
    'sample-root': { type: 'string' },
    sample: { type: 'string' },
    'render-mode': { type: 'string', default: 'shared' },
    'render-workload': { type: 'string', default: 'benchmark' },
    'render-large': { type: 'boolean', default: false },
  },
});
assert.equal(process.platform, 'win32', '当前真实 WebView 回归仅验证 Windows。');
const expectedDisplay = preflightOptions(options);
assert(['main', 'auxiliary'].includes(options['canvas-memory-focus']));
assert(
  options.scenario === 'canvas-memory' || options['canvas-memory-focus'] === 'main',
  '辅助窗口前台仅用于资源对照。',
);
assert(
  [
    'baseline',
    'canvas-inspection',
    'canvas-display',
    'canvas-memory',
    'render-lab',
    'render-benchmark',
    'geometry-25',
    'geometry-100',
    'geometry-200',
    'cleanup',
    'cleanup-max',
    'cleanup-expiry',
    'cleanup-recovery',
    'cleanup-late',
    'cleanup-window',
    'input-pan',
    'input-chord',
    'input-snapping',
    'window-controls',
    'window-blur',
    'window-menu',
    'window-exit',
  ].includes(options.scenario),
);
assert(
  options.driver,
  '用法：npm run test:dev-mcp:webview -- --driver <msedgedriver.exe> [--app <layerlens.exe>]',
);
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const python = checkDevPython();
const app = path.resolve(root, options.app ?? 'target/debug/layerlens.exe');
const driverPath = path.resolve(root, options.driver);
const base = path.resolve(root, options['output-root'] ?? '.local/dev-mcp');
await mkdir(base, { recursive: true });
const run = await mkdtemp(path.join(base, 'webview-'));
const runs = path.join(run, 'instances');
await mkdir(runs);
const isBenchmark = options.scenario === 'render-benchmark';
const isRender = options.scenario.startsWith('render-');
const isWorkspaceCanvas = options.scenario.startsWith('canvas-');
const nativeWindows =
  isBenchmark ||
  isWorkspaceCanvas ||
  options.scenario.startsWith('window-') ||
  options.scenario === 'cleanup-window';
assert(
  nativeWindows || (expectedDisplay.size === null && expectedDisplay.dpr === null),
  '显式显示条件仅用于原生窗口场景。',
);
assert(!options['sample-root'] || isBenchmark);
assert(!options.sample || isBenchmark);
assert(
  [
    'benchmark',
    'visual',
    'memory',
    'workspace',
    'project',
    'project-memory',
    'environment',
  ].includes(options['render-workload']),
);
assert(
  !options['render-large'] || (isBenchmark && options['render-workload'].startsWith('project')),
);
assert(['shared', 'native'].includes(options['render-mode']));
const sampleRoot = path.resolve(
  root,
  options['sample-root'] ?? 'crates/layerlens-core/tests/fixtures/psd',
);
const samples = isBenchmark
  ? [options.sample ?? 'render-simple.psd']
  : [
      options.scenario === 'render-lab' || isWorkspaceCanvas
        ? 'render-simple.psd'
        : 'viewer-scale.psd',
      'bitmap-rle.psd',
    ];
assert(samples.every((name) => path.basename(name) === name && name.endsWith('.psd')));
const sourceFile = await open(path.join(sampleRoot, samples[0]), 'r');
const sourceHeader = Buffer.alloc(26);
try {
  assert.equal((await sourceFile.read(sourceHeader, 0, 26, 0)).bytesRead, 26);
} finally {
  await sourceFile.close();
}
async function hash(file) {
  const h = createHash('sha256');
  for await (const c of createReadStream(file)) h.update(c);
  return h.digest('hex');
}
const sourceHashes = await Promise.all(samples.map((s) => hash(path.join(sampleRoot, s))));
const report = {
  python,
  evidence: 'webview-webdriver-mcp',
  scenario: options.scenario,
  memoryForeground:
    options.scenario === 'canvas-memory' ? options['canvas-memory-focus'] : undefined,
  passed: false,
  phase: 'driver-start',
  checks: [],
  actions: [],
  images: [],
  samples,
  sourceHashes,
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
  appSha256: await hash(app),
  driverSha256: await hash(driverPath),
  cargoLockSha256: await hash(path.join(root, 'Cargo.lock')),
  runnerSha256: Object.fromEntries(
    await Promise.all(
      [
        'test-dev-webview.mjs',
        'dev-mcp-process-monitor.mjs',
        'dev-mcp-render-focus.mjs',
        'dev-mcp-preflight.mjs',
        'dev-mcp-display-webview.mjs',
        'dev-mcp-workspace-canvas.mjs',
        'dev-mcp-workspace-memory.mjs',
        'dev-mcp-memory.mjs',
        'render-statistics.mjs',
        'dev-mcp-m2-webview.mjs',
        'dev-mcp-input-webview.mjs',
        'dev-mcp-cleanup-webview.mjs',
        'dev-mcp-window-webview.mjs',
        'check-composite-capture.py',
        'dev-mcp-render-environment.mjs',
        'dev-mcp-render-acceptance.mjs',
        'dev-mcp-render-benchmark.mjs',
        'dev-mcp-render-project.mjs',
        'dev-mcp-capture.mjs',
        'dev-python.mjs',
      ].map(async (name) => [name, await hash(path.join(root, 'scripts', name))]),
    ),
  ),
  system: {
    platform: process.platform,
    release: os.release(),
    arch: process.arch,
    node: process.version,
  },
  startedAt: new Date().toISOString(),
};
const started = performance.now();
const driver = spawn(
  driverPath,
  ['--port=0', '--allowed-ips=127.0.0.1', '--log-path=' + path.join(run, 'driver-service.log')],
  {
    windowsHide: true,
    stdio: ['ignore', 'pipe', 'pipe'],
  },
);
let driverLog = '',
  driverExited = false,
  endpoint,
  session,
  connection,
  info,
  display,
  frontend,
  monitor;
let rpcId = 0,
  mutationId = 0;
driver.once('exit', (code, signal) => {
  driverExited = true;
  report.driverExit = { code, signal, at: new Date().toISOString(), phase: report.phase };
});
driver.once('error', (error) => {
  report.driverError = errorDiagnostic(error);
});
const driverExit = new Promise((resolve) => {
  driver.once('error', resolve);
  driver.once('close', resolve);
});
for (const stream of [driver.stdout, driver.stderr])
  stream.on('data', (c) => {
    driverLog = (driverLog + c.toString()).slice(-64 * 1024);
  });
function deadline(promise, ms, label) {
  let timer;
  return Promise.race([
    promise,
    new Promise((_, reject) => {
      timer = setTimeout(() => reject(new Error(label)), ms);
    }),
  ]).finally(() => clearTimeout(timer));
}
const port = deadline(
  new Promise((resolve, reject) => {
    driver.once('error', reject);
    driver.once('exit', (code) => reject(new Error('驱动启动退出：' + code)));
    for (const stream of [driver.stdout, driver.stderr])
      stream.on('data', () => {
        const match = driverLog.match(/started successfully on port (\d+)/i);
        if (match) resolve(Number(match[1]));
      });
  }),
  10000,
  '驱动启动超时',
);
port.catch(() => undefined);
async function wd(route, body, method = 'POST') {
  const r = await fetch(endpoint + route, {
    method,
    headers: { 'Content-Type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
    signal: AbortSignal.timeout(30000),
  });
  const result = await r.json();
  assert(r.ok && !result.value?.error, JSON.stringify(result));
  return result.value;
}
async function readConnection() {
  const controller = new AbortController();
  const events = watch(runs, {
    recursive: true,
    signal: AbortSignal.any([controller.signal, AbortSignal.timeout(30000)]),
  });
  let next = events.next();
  next.catch(() => undefined);
  try {
    for (;;) {
      for (const dir of await readdir(runs, { withFileTypes: true })) {
        if (!dir.isDirectory()) continue;
        const directory = path.join(runs, dir.name);
        if (!(await readdir(directory)).includes('ready')) continue;
        const value = JSON.parse(await readFile(path.join(directory, 'connection.json'), 'utf8'));
        assert.equal(value.formatVersion, 1);
        assert(Number.isInteger(value.processId) && value.processId > 0);
        const url = new URL(value.endpoint);
        assert.equal(url.hostname, '127.0.0.1');
        assert.equal(url.pathname, '/mcp');
        return { ...value, directory };
      }
      await next;
      next = events.next();
      next.catch(() => undefined);
    }
  } finally {
    controller.abort();
    await events.return().catch((e) => {
      if (e.name !== 'AbortError') throw e;
    });
  }
}
async function rpc(method, params, notification = false) {
  const id = notification ? undefined : ++rpcId;
  const action = {
    id,
    method,
    tool: params?.name,
    requestId: params?.arguments?.clientRequestId,
    phase: report.phase,
    startedAt: new Date().toISOString(),
  };
  report.actions.push(action);
  const requestStarted = performance.now();
  try {
    const response = await fetch(connection.endpoint, {
      method: 'POST',
      headers: {
        Authorization: 'Bearer ' + connection.token,
        'Content-Type': 'application/json',
        Accept: 'application/json, text/event-stream',
        'MCP-Protocol-Version': '2025-11-25',
      },
      body: JSON.stringify({ jsonrpc: '2.0', id, method, params }),
      signal: AbortSignal.timeout(35000),
    });
    action.httpStatus = response.status;
    assert(response.ok, 'MCP HTTP ' + response.status);
    if (notification) return;
    const result = await response.json();
    assert.equal(result.id, id);
    assert.equal(result.error, undefined, JSON.stringify(result.error));
    action.isError = result.result.isError;
    action.errorCode = result.result.structuredContent?.data?.error?.code;
    return result.result;
  } catch (error) {
    action.transportError = errorDiagnostic(error);
    throw error;
  } finally {
    action.durationMs = performance.now() - requestStarted;
  }
}
const args = (request, mutate = false) => ({
  instanceId: info.instanceId,
  sessionId: info.sessionId,
  clientRequestId: mutate ? 'webview-' + ++mutationId : null,
  request,
});
async function tool(name, input, expectedError = false) {
  const r = await rpc('tools/call', { name, arguments: input });
  assert.equal(r.isError, expectedError, JSON.stringify(r.structuredContent));
  assert.deepEqual(JSON.parse(r.content[0].text), r.structuredContent);
  return r.structuredContent.data;
}
const state = async () => (await tool('dev_ui_state', args({}))).value;
const core = async () =>
  (await tool('dev_workspace', args({ protocolVersion: 1, action: { kind: 'snapshot' } }))).value
    .snapshot;
const workspace = async (action) =>
  (await tool('dev_workspace', args({ protocolVersion: 1, action }, true))).value;
const selection = (operation, mutation = true) =>
  tool(
    'dev_selection',
    args({ protocolVersion: 1, sessionId: info.sessionId, operation }, mutation),
  );
async function wait(condition) {
  const r = (
    await tool(
      'dev_ui_wait',
      args({
        frontendId: frontend ?? null,
        condition,
        timeoutMs: options['render-large'] ? 30000 : 10000,
      }),
    )
  ).value;
  assert.equal(r.satisfied, true, JSON.stringify(r));
  return r.lastState;
}
async function waitCore(condition) {
  if (condition.kind === 'frontendImageReady')
    condition = { ...condition, atLeast: (await core()).sequence };
  const r = (await tool('dev_wait', args({ condition, timeoutMs: 10000 }))).value;
  assert.equal(r.satisfied, true, JSON.stringify(r));
  return r.lastState;
}
async function action(input, expectedError = false) {
  const result = await tool(
    'dev_ui_action',
    args({ frontendId: frontend, expectedViewRevision: null, action: input }, true),
    expectedError,
  );
  report.inputs ??= [];
  report.inputs.push({ input, before: result.value?.before, error: result.error });
  return result;
}
const click = (target) => action({ kind: 'click', target });
const key = (key, phase = 'press') => action({ kind: 'key', key, phase });
const inspectDialog = () =>
  wd('/session/' + session + '/execute/sync', {
    script:
      "const dialog=document.querySelector('dialog[open]'); const list=dialog?.querySelector('.cleanup-tasks'); return {focusInside:!!dialog?.contains(document.activeElement),taskCount:list?.children.length??0,scrollable:!!list&&list.scrollHeight>list.clientHeight};",
    args: [],
  });
function point(view, x, y) {
  const r = view.canvas.image;
  return { x: Math.round(r.x + x * view.canvas.zoom), y: Math.round(r.y + y * view.canvas.zoom) };
}
async function screenshot(name, tiled = options['render-large']) {
  if (tiled) {
    const { data, capture } = await captureTiled({ rpc, args, frontend, run, root, name });
    report.images.push({
      name,
      before: data.before.viewRevision,
      after: data.after.viewRevision,
      unchanged: data.unchanged,
      canvas: data.before.canvas,
      dpr: data.before.window.devicePixelRatio,
      engine: data.before.renderLab?.engine,
      generation: data.before.renderLab?.generation,
      sourceSha256: data.before.renderLab?.sourceSha256,
      profile: data.before.renderLab?.profile,
      rgbaSha256: capture.rgbaSha256,
      tiles: capture.tileCount,
    });
    return;
  }
  const result = await rpc('tools/call', {
    name: 'dev_ui_screenshot',
    arguments: args({ frontendId: frontend }),
  });
  assert.equal(result.isError, false, JSON.stringify(result.structuredContent));
  const data = result.structuredContent.data.value;
  assert.equal(data.source, 'webdriverWebview');
  assert.equal(data.before.frontendId, frontend);
  if (isRender) assert.equal(data.unchanged, true, '截图期间实验观测已变化');
  const png = Buffer.from(result.content.find((c) => c.type === 'image').data, 'base64');
  assert.equal(png.readUInt32BE(16), data.width);
  assert.equal(png.readUInt32BE(20), data.height);
  assert(data.width >= 900 && data.height >= 600);
  await writeFile(path.join(run, name), png, { flag: 'wx' });
  report.images.push({
    name,
    before: data.before.viewRevision,
    after: data.after.viewRevision,
    unchanged: data.unchanged,
    ...(isRender
      ? {
          canvas: data.before.canvas,
          dpr: data.before.window.devicePixelRatio,
          engine: data.before.renderLab?.engine,
          generation: data.before.renderLab?.generation,
          sourceSha256: data.before.renderLab?.sourceSha256,
          profile: data.before.renderLab?.profile,
        }
      : {}),
  });
}
try {
  endpoint = 'http://127.0.0.1:' + (await port);
  const config = path.join(run, 'config.json');
  await writeFile(
    config,
    JSON.stringify({
      sampleRoot,
      runRoot: runs,
      visible: options.visible || nativeWindows,
      webDriverUrl: endpoint,
      nativeWindows,
      renderLab: isRender,
      largeRender: options['render-large'],
    }),
    { flag: 'wx' },
  );
  report.phase = 'webview-start';
  const launched = await wd('/session', {
    capabilities: {
      alwaysMatch: {
        browserName: 'webview2',
        'ms:edgeOptions': {
          binary: app,
          args: ['--dev-mcp-config=' + config],
          windowTypes: ['webview'],
        },
      },
    },
  });
  session = launched.sessionId;
  report.capabilities = launched.capabilities;
  connection = await readConnection();
  monitor = observeProcess(connection.processId);
  await deadline(monitor.ready, 10000, '进程监测握手超时');
  report.processId = connection.processId;
  await rpc('initialize', {
    protocolVersion: '2025-11-25',
    capabilities: {},
    clientInfo: { name: 'layerlens-webview-regression', version: '1' },
  });
  await rpc('notifications/initialized', {}, true);
  info = (await tool('dev_instance', {})).value;
  assert.equal(info.instanceId, connection.instanceId);
  report.instanceId = info.instanceId;
  report.buildProfile = info.buildProfile;
  if (options.scenario === 'canvas-memory')
    assert.equal(report.buildProfile, 'release', '资源验收必须使用 release 开发测试程序');
  const expectedTools = nativeWindows ? 20 : 17;
  assert.equal((await rpc('tools/list', {})).tools.length, expectedTools);
  let view = await wait({ kind: 'ready' });
  frontend = view.frontendId;
  report.frontendId = frontend;
  report.driver = (await tool('dev_ui_attach', args({ driverSessionId: session }, true))).value;
  if (isBenchmark) display = createDisplayGuard({ state, tool, args }, report);
  assert.equal(report.driver.browserVersion, launched.capabilities.browserVersion);
  assert.equal(report.driver.driverVersion.split(' ')[0], launched.capabilities.browserVersion);
  if (nativeWindows) await prepareTestWindow({ state, tool, args, wait, report }, expectedDisplay);
  // 固定的只读事件诊断，不生成输入、不更改组件或核心状态。
  await wd('/session/' + session + '/execute/sync', {
    script: `
    const trace=[];
    for(const type of ['pointerdown','pointermove','pointerup','pointercancel','lostpointercapture','gotpointercapture','blur','dragstart','dragend']) {
      window.addEventListener(type,event=>{ trace.push({type:event.type,buttons:event.buttons,pointerId:event.pointerId,trusted:event.isTrusted,target:event.target?.dataset?.devTarget??event.target?.tagName}); if(trace.length>32)trace.shift(); },true);
    }
    Object.defineProperty(window,'__LAYERLENS_INPUT_TRACE__',{value:()=>trace.slice()});
  `,
    args: [],
  });
  report.checks.push(`匹配驱动、WebView 身份、前端握手及 ${expectedTools} 个真实工具`);
  report.phase = 'document-sync';
  if (isBenchmark) {
    if (options['render-mode'] === 'native') await click('render:native');
    report.beforeOpenMemory = (await tool('dev_process_metrics', args({}))).value;
  }
  if (options.scenario === 'canvas-memory') {
    report.beforeOpenMemory = (await tool('dev_process_metrics', args({}))).value;
    report.beforeOpenResources = (await state()).previewResources;
  }
  const openStarted = performance.now();
  await workspace({
    kind: 'open',
    path: options.scenario.startsWith('cleanup') ? samples[1] : samples[0],
  });
  let workspaceState = await waitCore({ kind: 'documentCount', count: 1 });
  const a = workspaceState.documents[0];
  await wait({ kind: 'applied', atLeast: workspaceState.sequence });
  view = await wait({ kind: 'imageReady', documentId: a.id, revision: a.revision });
  if (isBenchmark) report.openToObservedMs = performance.now() - openStarted;
  assert.equal(view.sessionId, info.sessionId);
  assert.equal(
    view.canvas.documentWidth,
    isBenchmark
      ? sourceHeader.readUInt32BE(18)
      : options.scenario === 'render-lab' || isWorkspaceCanvas
        ? 480
        : options.scenario.startsWith('cleanup')
          ? 4
          : 256,
  );
  assert.equal(
    view.canvas.documentHeight,
    isBenchmark
      ? sourceHeader.readUInt32BE(14)
      : options.scenario === 'render-lab' || isWorkspaceCanvas
        ? 320
        : options.scenario.startsWith('cleanup')
          ? 3
          : 128,
  );
  if (isWorkspaceCanvas) {
    await exerciseWorkspaceCanvas({
      scenario: options.scenario,
      state,
      core,
      wait,
      waitCore,
      action,
      click,
      key,
      point,
      selection,
      workspace,
      tool,
      args,
      report,
      screenshot,
      document: a,
      checkpoint: async () =>
        writeFile(
          path.join(run, 'report-progress.json'),
          redact(JSON.stringify(report, null, 2), [connection?.token]),
        ),
      verifyComposite: async (name, view) => {
        let evidence;
        try {
          evidence = runDevPython(
            [path.join(root, 'scripts/check-composite-capture.py'), path.join(run, name)],
            {
              cwd: root,
              windowsHide: true,
              encoding: 'utf8',
              input: JSON.stringify({ canvas: view.canvas, dpr: view.window.devicePixelRatio }),
            },
          );
        } catch (cause) {
          if (cause.stdout?.length)
            (report.visualChecks ??= []).push({ name, ...JSON.parse(cause.stdout.toString()) });
          throw cause;
        }
        (report.visualChecks ??= []).push({ name, ...JSON.parse(evidence) });
      },
    });
  } else if (isBenchmark) {
    await (
      options['render-workload'] === 'environment'
        ? exerciseDisplayEnvironment
        : options['render-workload'].startsWith('project')
          ? exerciseRenderProject
          : options['render-workload'] === 'benchmark'
            ? exerciseRenderBenchmark
            : exerciseRenderAcceptance
    )({
      state,
      core,
      wait,
      waitCore,
      action,
      click,
      key,
      point,
      tool,
      args,
      workspace,
      report,
      screenshot,
      document: a,
      display,
      sample: samples[0],
      textMode: options['render-mode'],
      workload: options['render-workload'],
      projectSpec: options['render-large']
        ? JSON.parse(await readFile(path.join(root, '.local/render-tab1/manifest.json'), 'utf8'))
        : null,
      checkpoint: async () =>
        writeFile(
          path.join(run, 'report-progress.json'),
          redact(JSON.stringify(report, null, 2), [connection?.token]),
        ),
    });
  } else if (options.scenario === 'render-lab') {
    await exerciseRenderLab({
      state,
      core,
      wait,
      waitCore,
      action,
      click,
      key,
      point,
      tool,
      args,
      workspace,
      report,
      screenshot,
      document: a,
    });
  } else if (options.scenario === 'baseline') {
    report.phase = 'menu-and-draft';
    await click('menu:window');
    await wait({ kind: 'menu', menu: 'window' });
    const waitingPanel = wait({ kind: 'panel', panel: 'tasks' });
    await click('command:tasks');
    await waitingPanel;
    await click('tool:region');
    await click('canvas:snap');
    view = await state();
    const p0 = point(view, 17, 17),
      p1 = point(view, 47, 39);
    const originalRevision = view.selectionRevision;
    await action({ kind: 'drag', startX: p0.x, startY: p0.y, endX: p1.x, endY: p1.y, hold: true });
    const draft = await wait({ kind: 'draft', active: true });
    assert.equal(draft.selectionRevision, originalRevision);
    assert.equal((await core()).selection.selectionRevision, originalRevision);
    await key('escape');
    await wait({ kind: 'draft', active: false });
    await action({ kind: 'releaseInputs' });
    assert.equal((await core()).selection.selectionRevision, originalRevision);
    await action({ kind: 'drag', startX: p0.x, startY: p0.y, endX: p1.x, endY: p1.y, hold: false });
    view = await wait({ kind: 'selection', after: originalRevision, hasScope: true });
    for (const [field, expected] of Object.entries({ x: 17, y: 17, width: 30, height: 22 }))
      assert(Math.abs(view.region[field] - expected) <= 1, field + '：' + view.region[field]);
    workspaceState = await core();
    assert.equal(workspaceState.selection.scope.targetCount, 12);
    const snapshotId = view.snapshotId,
      selectionRevision = view.selectionRevision;
    await click('menu:file');
    await wait({ kind: 'menu', menu: 'file' });
    await key('escape');
    await wait({ kind: 'menu', menu: null });
    assert.equal((await core()).selection.selectionRevision, selectionRevision);
    assert(BigInt((await state()).input.pointerEvents) > 0n);
    assert(BigInt((await state()).input.keyEvents) > 0n);
    report.checks.push('真实菜单输入、并发前端等待、草稿不提交、Esc 取消与 12 个独立网格命中预期');
    report.phase = 'task-panel';
    await action({ kind: 'fillTaskName', text: 'WebView 固定范围 A' });
    await click('task:create');
    view = await wait({ kind: 'taskCount', count: 1 });
    const task = view.tasks[0];
    assert.equal(task.snapshotId, snapshotId);
    await click('task:' + task.id + ':view');
    await wait({ kind: 'content', snapshotId });
    await screenshot('fixed-task.png');
    report.phase = 'snapping-and-alt';
    await click('canvas:snap');
    view = await state();
    const s0 = point(view, 65, 17),
      s1 = point(view, 95, 39),
      beforeSnap = view.selectionRevision;
    await action({ kind: 'drag', startX: s0.x, startY: s0.y, endX: s1.x, endY: s1.y, hold: true });
    view = await wait({ kind: 'draft', active: true });
    assert.equal(view.canvas.snapGuides, 2);
    assert(Math.abs(view.canvas.draft.x + view.canvas.draft.width - 96) < 0.01);
    assert(Math.abs(view.canvas.draft.y + view.canvas.draft.height - 40) < 0.01);
    let keyResult = await key('alt', 'down');
    view = await wait({
      kind: 'canvasModifiers',
      alt: true,
      ctrl: false,
      keyEventsAtLeast: (BigInt(keyResult.value.before.input.keyEvents) + 1n).toString(),
    });
    assert.equal(view.canvas.alt, true);
    assert.equal(view.canvas.snapGuides, 0);
    keyResult = await key('alt', 'up');
    view = await wait({
      kind: 'canvasModifiers',
      alt: false,
      ctrl: false,
      keyEventsAtLeast: (BigInt(keyResult.value.before.input.keyEvents) + 1n).toString(),
    });
    assert.equal(view.canvas.alt, false);
    assert.equal(view.canvas.snapGuides, 2);
    await key('escape');
    await wait({ kind: 'draft', active: false });
    await action({ kind: 'releaseInputs' });
    assert.equal((await core()).selection.selectionRevision, beforeSnap);
    await action({ kind: 'drag', startX: s0.x, startY: s0.y, endX: s1.x, endY: s1.y, hold: false });
    view = await wait({ kind: 'selection', after: beforeSnap, hasScope: true });
    assert(Math.abs(view.region.x + view.region.width - 96) < 0.01);
    assert(Math.abs(view.region.y + view.region.height - 40) < 0.01);
    assert.notEqual(view.snapshotId, snapshotId);
    assert.equal(view.tasks[0].snapshotId, snapshotId);
    report.checks.push('真实几何边缘吸附、Alt 临时关闭／恢复、取消与改选不改变固定任务');
    report.phase = 'm2-reload';
    const beforeReload = await core();
    await click('menu:file');
    await click('command:reload');
    await waitCore({ kind: 'sequence', atLeast: (BigInt(beforeReload.sequence) + 1n).toString() });
    const reloaded = await waitCore({ kind: 'idle' });
    const newRevision = reloaded.documents.find((d) => d.id === a.id).revision;
    assert.notEqual(newRevision, a.revision);
    view = await wait({ kind: 'imageReady', documentId: a.id, revision: newRevision });
    assert.equal(view.region, null);
    assert.equal(view.snapshotId, null);
    const retained = await selection(
      { kind: 'content', target: { kind: 'task', taskId: task.id }, cursor: null, limit: 32 },
      false,
    );
    assert.equal(retained.value.page.documentRevision, a.revision);
    assert.equal(retained.value.page.snapshotId, snapshotId);
    a.revision = newRevision;
    report.checks.push('真实菜单重载清空当前范围，固定任务仍读取原修订');
    report.phase = 'task-panel';
    await workspace({ kind: 'open', path: samples[1] });
    workspaceState = await waitCore({ kind: 'documentCount', count: 2 });
    const b = workspaceState.documents.find((d) => d.id !== a.id);
    await wait({ kind: 'imageReady', documentId: b.id, revision: b.revision });
    await click('document:' + a.id);
    await wait({ kind: 'imageReady', documentId: a.id, revision: a.revision });
    await click('menu:file');
    await click('command:close');
    workspaceState = await waitCore({ kind: 'documentCount', count: 1 });
    await wait({ kind: 'applied', atLeast: workspaceState.sequence });
    await click('task:' + task.id + ':view');
    view = await wait({ kind: 'content', snapshotId });
    assert.equal(view.activeDocument.id, b.id);
    report.checks.push('任务面板真实键入／固定／读取，以及 UI 关闭标签后保留原快照');
    report.phase = 'cleanup-dialog';
    const beforeCleanup = (await core()).selection.selectionRevision;
    await click('task:' + task.id + ':cleanup');
    view = await wait({ kind: 'cleanup', phase: 'ready' });
    assert.deepEqual(view.cleanup.taskIds, [task.id]);
    assert.equal(view.focus, 'cleanup-title');
    await key('escape');
    await wait({ kind: 'cleanup', phase: null });
    assert.equal((await core()).selection.selectionRevision, beforeCleanup);
    assert.equal((await state()).tasks[0].status, 'active');
    await click('task:' + task.id + ':cleanup');
    await wait({ kind: 'cleanup', phase: 'ready' });
    await screenshot('cleanup-confirmation.png');
    const confirm = args(
      {
        frontendId: frontend,
        expectedViewRevision: null,
        action: { kind: 'click', target: 'cleanup:confirm' },
      },
      true,
    );
    const first = await tool('dev_ui_action', confirm);
    await wait({ kind: 'cleanup', phase: 'complete' });
    view = await wait({ kind: 'task', taskId: task.id, status: 'invalidated' });
    assert.equal(view.content, null);
    assert.equal(view.activeDocument.id, b.id);
    assert.deepEqual(await tool('dev_ui_action', confirm), first);
    await click('cleanup:cancel');
    await wait({ kind: 'cleanup', phase: null });
    report.checks.push('真实清理模态、焦点、Esc 隔离、确认幂等、任务失效及前端同步');
    report.phase = 'frontend-navigation';
    const oldFrontend = frontend;
    const disconnected = tool(
      'dev_ui_wait',
      args({
        frontendId: oldFrontend,
        condition: { kind: 'viewRevision', atLeast: '1000000' },
        timeoutMs: 10000,
      }),
      true,
    );
    await wd('/session/' + session + '/refresh', {});
    const terminated = await disconnected;
    assert(['NOT_FOUND', 'STALE_REVISION'].includes(terminated.error.code));
    frontend = undefined;
    view = await wait({ kind: 'ready' });
    frontend = view.frontendId;
    assert.notEqual(frontend, oldFrontend);
    assert.equal(view.sessionId, info.sessionId);
    assert.equal(view.activeDocument.id, b.id);
    await wait({ kind: 'imageReady', documentId: b.id, revision: b.revision });
    await click('panel:tasks');
    await wait({ kind: 'task', taskId: task.id, status: 'invalidated' });
    report.checks.push('真实页面刷新使旧等待失效，新前端代次重连同一核心会话');
    report.phase = 'negative-and-screenshot';
    const stale = await tool(
      'dev_ui_action',
      args(
        {
          frontendId: 'stale-page',
          expectedViewRevision: null,
          action: { kind: 'click', target: 'tool:pan' },
        },
        true,
      ),
      true,
    );
    assert.equal(stale.error.code, 'STALE_REVISION');
    const rejected = await tool(
      'dev_ui_action',
      args(
        {
          frontendId: frontend,
          expectedViewRevision: null,
          action: { kind: 'click', target: 'body' },
        },
        true,
      ),
      true,
    );
    assert.equal(rejected.error.code, 'INVALID_INPUT');
    const timeout = (
      await tool(
        'dev_ui_wait',
        args({
          frontendId: frontend,
          condition: { kind: 'task', taskId: task.id, status: 'active' },
          timeoutMs: 1,
        }),
      )
    ).value;
    assert.equal(timeout.timedOut, true);
    assert.equal(timeout.lastState.tasks[0].status, 'invalidated');
    await screenshot('generic-capture.png', true);
    report.checks.push('普通开发实例通过 MCP 固定捕获、分块读回、RGBA 指纹核验和显式释放');
    await screenshot('final-state.png');
    assert.deepEqual(
      await Promise.all(samples.map((s) => hash(path.join(sampleRoot, s)))),
      sourceHashes,
    );
    report.checks.push('旧前端／非法目标拒绝、超时末态、截图版本关联与源文件不变');
  } else if (options.scenario.startsWith('window-')) {
    report.windowResult = await exerciseWindows({
      scenario: options.scenario,
      state,
      core,
      wait,
      waitCore,
      action,
      click,
      key,
      point,
      selection,
      workspace,
      tool,
      args,
      report,
      screenshot,
      document: a,
    });
  } else if (options.scenario.startsWith('geometry-')) {
    const zoom = Number(options.scenario.slice('geometry-'.length)) / 100;
    await exerciseRegionControls({
      state,
      core,
      wait,
      action,
      click,
      key,
      point,
      selection,
      report,
      document: a,
      zoom,
    });
    await screenshot('final-state.png');
  } else if (options.scenario.startsWith('input-')) {
    await exerciseCanvasInput({
      scenario: options.scenario,
      state,
      core,
      wait,
      action,
      click,
      key,
      point,
      selection,
      report,
      document: a,
    });
    await screenshot('final-state.png');
  } else if (options.scenario.startsWith('cleanup-')) {
    await exerciseCleanupBoundaries({
      scenario: options.scenario,
      state,
      core,
      wait,
      waitCore,
      click,
      key,
      selection,
      workspace,
      tool,
      args,
      sessionId: info.sessionId,
      report,
      screenshot,
      document: a,
    });
    await screenshot('final-state.png');
  } else {
    await exerciseOpenCleanup({
      core,
      state,
      wait,
      waitCore,
      click,
      key,
      selection,
      report,
      document: a,
      inspectDialog,
      screenshot,
    });
    await screenshot('final-state.png');
  }
  report.finalView = await state();
  report.finalCore = await core();
  if (display) await display.finish('success');
  if (isWorkspaceCanvas)
    await recordWorkspaceEnvironment({ state, tool, args, report }, 'terminal-success', null, true);
  assert.deepEqual(
    await Promise.all(samples.map((s) => hash(path.join(sampleRoot, s)))),
    sourceHashes,
  );
  report.phase = 'shutdown';
  if (report.windowResult?.closeWindow) {
    // 关闭导致 WebDriver 断连时保留原因；是否正常退出只由外部进程句柄判定。
    try {
      await click('window:close');
    } catch (error) {
      report.closeResponseError = String(error);
    }
  } else await tool('dev_shutdown', args({}));
  assert.equal(await deadline(monitor.exit, 10000, '应用正常退出超时'), 0);
  report.exitCode = 0;
  assert.deepEqual(
    await Promise.all(samples.map((s) => hash(path.join(sampleRoot, s)))),
    sourceHashes,
  );
  const remaining = await readdir(connection.directory);
  assert(remaining.includes('stopped'));
  assert(!remaining.includes('ready') && !remaining.includes('connection.json'));
  report.checks.push('真实应用退出码 0 与连接文件回收');
  report.passed = true;
  report.phase = 'complete';
} catch (error) {
  report.failureCategory =
    report.preflight && !report.preflight.passed ? 'environment-precondition' : 'scenario';
  report.error = error.stack ?? String(error);
  report.errorDetails = errorDiagnostic(error);
  report.processAtFailure = monitor?.snapshot();
  if (isWorkspaceCanvas && frontend) {
    try {
      report.failureView = await state();
      report.failureCore = await core();
    } catch (cause) {
      report.failureStateError = String(cause);
    }
    try {
      await recordWorkspaceEnvironment(
        { state, tool, args, report },
        'terminal-failure',
        null,
        true,
      );
    } catch (cause) {
      report.workspaceTerminalError = String(cause);
    }
  }
  if (display) {
    try {
      await display.finish('failure');
    } catch (cause) {
      report.displayTerminalError = String(cause);
    }
  }
  if (session) {
    try {
      report.inputTrace = await wd('/session/' + session + '/execute/sync', {
        script: 'return window.__LAYERLENS_INPUT_TRACE__?.() ?? [];',
        args: [],
      });
    } catch (cause) {
      report.inputTraceError = String(cause);
    }
    try {
      report.browserLog = await wd('/session/' + session + '/log', { type: 'browser' });
    } catch (cause) {
      report.browserLogError = String(cause);
    }
  }
  process.exitCode = 1;
  if (info && frontend)
    try {
      await screenshot('failure.png');
    } catch (cause) {
      report.captureError = String(cause);
    }
} finally {
  report.processBeforeShutdown = monitor?.snapshot();
  if (!report.passed && info) {
    try {
      await tool('dev_shutdown', args({}));
    } catch (error) {
      report.shutdownError = String(error);
      report.shutdownErrorDetails = errorDiagnostic(error);
    }
  }
  if (monitor) {
    try {
      report.observedExitCode = await deadline(monitor.exit, 10000, '应用退出观测超时');
    } catch (error) {
      report.processObservationError = errorDiagnostic(error);
    }
    report.processBeforeDriverCleanup = monitor.snapshot();
  }
  if (session)
    try {
      await wd('/session/' + session, undefined, 'DELETE');
    } catch (error) {
      report.driverCleanup = String(error);
    }
  report.processAfterDriverCleanup = monitor?.snapshot();
  if (monitor?.child.exitCode === null) {
    report.monitorStoppedByRunner = true;
    monitor.child.kill();
  }
  if (!driverExited) driver.kill();
  try {
    await deadline(driverExit, 5000, '驱动进程未退出');
  } catch (error) {
    report.driverExitError = String(error);
    report.passed = false;
    process.exitCode = 1;
  }
  report.durationMs = Math.round(performance.now() - started);
  await writeFile(path.join(run, 'driver.log'), redact(driverLog, [connection?.token]), {
    flag: 'wx',
  });
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
        phase: report.phase,
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
