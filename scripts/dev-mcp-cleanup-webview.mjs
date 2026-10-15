// 清理边界通过真实 MCP、WebView 输入和命名检查点验证；没有固定休眠或组件状态写入。
import assert from 'node:assert/strict';

export async function exerciseCleanupBoundaries(context) {
  const {
    scenario,
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
    report,
    screenshot,
  } = context;
  let document = context.document;
  const control = async (request) =>
    (
      await tool(
        'dev_test_control',
        args(request, ['arm', 'advancePlanClock'].includes(request.kind)),
      )
    ).value;
  const arm = async (checkpoint, mode = 'pause') =>
    (await control({ kind: 'arm', checkpoint, mode, timeoutMs: 30000 })).gate.id;
  const reached = async (id, phase = 'reached') => {
    const result = await control({ kind: 'wait', id, phase, timeoutMs: 10000 });
    assert.equal(result.satisfied, true, JSON.stringify(result));
    report.checkpoints ??= [];
    report.checkpoints.push(result.gate);
  };
  const release = (id) => control({ kind: 'release', id });
  const cleanup = async (operation) =>
    (
      await tool(
        'dev_cleanup',
        args({ protocolVersion: 1, sessionId: context.sessionId, operation }, true),
      )
    ).value;
  async function select() {
    const before = await core();
    const chosen = await selection({
      kind: 'region',
      documentId: document.id,
      documentRevision: document.revision,
      expectedRevision: before.selection.selectionRevision,
      bounds: { x: 0, y: 0, width: 4, height: 3 },
    });
    const applied = await wait({ kind: 'applied', atLeast: (await core()).sequence });
    assert.equal(applied.snapshotId, chosen.value.snapshotId);
    return chosen.value.snapshotId;
  }
  async function prepare(snapshotId, count) {
    const request = args({ snapshotId, count, namePrefix: '清理 <影响> & 任务' }, true);
    const first = await tool('dev_prepare_tasks', request);
    assert.equal(first.value.status, 'complete');
    assert.equal(first.value.error, null);
    assert.equal(first.value.taskIds.length, count);
    assert.equal(new Set(first.value.taskIds).size, count);
    assert.deepEqual(await tool('dev_prepare_tasks', request), first);
    return first.value.taskIds;
  }
  async function refresh() {
    await click('panel:tasks');
    await wait({ kind: 'selectionIdle' });
    await click('task:refresh');
    return wait({ kind: 'selectionIdle' });
  }
  async function preflight() {
    await click('menu:file');
    await click('command:cleanup');
    return wait({ kind: 'cleanup', phase: 'ready' });
  }
  async function externalCleanup() {
    const plan = await cleanup({
      kind: 'prepare',
      target: { kind: 'document', documentId: document.id, documentRevision: document.revision },
    });
    const result = await cleanup({ kind: 'confirm', planId: plan.plan.planId });
    const current = await waitCore({ kind: 'cleanup', atLeast: result.receipt.revision });
    await wait({ kind: 'applied', atLeast: current.sequence });
    return result.receipt;
  }
  function invalidated(view, ids) {
    for (const id of ids)
      assert.equal(
        view.tasks.find((task) => task.id === id)?.status,
        'invalidated',
        `任务 ${id} 必须存在且失效`,
      );
    assert.equal(view.content, null);
  }
  report.phase = scenario;
  const snapshotId = await select();
  if (scenario === 'cleanup-max') {
    const ids = await prepare(snapshotId, 128);
    await refresh();
    await wait({ kind: 'taskCount', count: 32 });
    for (const count of [64, 96, 128]) {
      await click('task:more');
      await wait({ kind: 'taskCount', count });
      await wait({ kind: 'selectionIdle' });
    }
    let view = await preflight();
    assert.deepEqual([...view.cleanup.taskIds].sort(), [...ids].sort());
    assert.equal(view.cleanup.renderedTasks, 128);
    assert.equal(view.cleanup.scrollable, true);
    await click('cleanup:tasks');
    await key('end');
    view = await wait({ kind: 'cleanupListEnd' });
    assert.equal(view.cleanup.focusInside, true);
    await screenshot('cleanup-confirmation.png');
    await key('escape');
    await wait({ kind: 'cleanup', phase: null });
    assert.equal((await core()).selection.scope.snapshotId, snapshotId);
    await preflight();
    await click('cleanup:confirm');
    await wait({ kind: 'cleanupSync', failed: false });
    view = await wait({ kind: 'selectionIdle' });
    assert.equal(view.tasks.length, 128);
    invalidated(view, ids);
    assert.equal((await core()).cleanup.invalidatedTaskIds.length, 128);
    await waitCore({ kind: 'resourcesReleased' });
    report.checks.push(
      '128 项准备及幂等重放、四页任务、完整 DOM 影响与键盘滚动到底、Esc 取消和全部失效',
    );
  } else if (scenario === 'cleanup-expiry') {
    const ids = await prepare(snapshotId, 2);
    await refresh();
    const old = (await preflight()).cleanup.planId;
    const revision = (await core()).cleanup.revision;
    await control({ kind: 'advancePlanClock', seconds: 301 });
    await click('cleanup:confirm');
    await wait({ kind: 'cleanup', phase: 'stale' });
    const unchanged = await core();
    assert.equal(unchanged.cleanup.revision, revision);
    assert.equal(unchanged.selection.scope.snapshotId, snapshotId);
    assert((await state()).tasks.every((task) => task.status === 'active'));
    await click('cleanup:recheck');
    assert.notEqual((await wait({ kind: 'cleanup', phase: 'ready' })).cleanup.planId, old);
    await click('cleanup:confirm');
    await wait({ kind: 'cleanupSync', failed: false });
    invalidated(await wait({ kind: 'selectionIdle' }), ids);
    await waitCore({ kind: 'resourcesReleased' });
    report.checks.push('推进清理计划时钟后旧确认无副作用拒绝，重新预检后成功');
  } else if (scenario === 'cleanup-recovery') {
    const ids = await prepare(snapshotId, 2);
    await refresh();
    const plan = (await preflight()).cleanup.planId;
    const notification = await arm('desktopNotification');
    const lost = await arm('desktopCleanupCommitted', 'fail');
    await click('cleanup:confirm');
    await reached(lost, 'failed');
    await reached(notification);
    const uncertain = await wait({ kind: 'cleanup', phase: 'uncertain' });
    assert.equal(uncertain.cleanup.planId, plan);
    const committed = await core();
    assert.equal(committed.documents.length, 0);
    assert.deepEqual(committed.cleanup.invalidatedTaskIds, ids);
    const sync = await arm('desktopSnapshot', 'fail');
    await click('cleanup:confirm');
    await reached(sync, 'failed');
    const failed = await wait({ kind: 'cleanupSync', failed: true });
    assert.equal(failed.cleanup.pending, true, '通知仍被保留，失败同步不能解除清理屏障');
    invalidated(failed, ids);
    assert.equal(failed.canvas, null);
    await screenshot('cleanup-sync-failure.png');
    await click('cleanup:sync');
    const restored = await wait({ kind: 'cleanupSync', failed: false });
    assert.equal(restored.cleanup.pending, false);
    assert.equal((await core()).cleanup.revision, committed.cleanup.revision);
    await release(notification);
    await wait({ kind: 'selectionIdle' });
    await workspace({ kind: 'open', path: 'bitmap-rle.psd' });
    const reopened = await waitCore({ kind: 'documentCount', count: 1 });
    assert.notEqual(reopened.documents[0].id, document.id);
    const replay = await cleanup({ kind: 'confirm', planId: plan });
    assert.equal(replay.receipt.revision, committed.cleanup.revision);
    assert.equal((await core()).documents[0].id, reopened.documents[0].id);
    await workspace({ kind: 'close', documentId: reopened.documents[0].id });
    await waitCore({ kind: 'resourcesReleased' });
    report.checks.push(
      '真实确认回包丢失后重试原计划、同步失败屏障与恢复、迟到通知、重开文档不被旧计划清理',
    );
  } else {
    assert.equal(scenario, 'cleanup-late');
    // 保留另一文档的任务，确认目标清理不会丢弃其读取。
    await workspace({ kind: 'open', path: 'bitmap-raw.psd' });
    const opened = await waitCore({ kind: 'documentCount', count: 2 });
    const other = opened.documents.find((entry) => entry.id !== document.id);
    const target = document;
    document = other;
    await workspace({ kind: 'activate', documentId: other.id });
    await waitCore({ kind: 'frontendImageReady', documentId: other.id, revision: other.revision });
    const otherSnapshot = await select();
    const [otherTask] = await prepare(otherSnapshot, 1);
    document = target;
    for (const point of ['desktopTasks', 'desktopContent', 'desktopTaskCreated']) {
      report.phase = `cleanup-late-${point}`;
      await workspace({ kind: 'activate', documentId: document.id });
      await waitCore({
        kind: 'frontendImageReady',
        documentId: document.id,
        revision: document.revision,
      });
      const fixed = await select();
      const ids = await prepare(fixed, 1);
      await refresh();
      const gate = await arm(point);
      if (point === 'desktopTasks') await click('task:refresh');
      else if (point === 'desktopContent') await click(`task:${ids[0]}:view`);
      else await click('task:create');
      await reached(gate);
      const receipt = await externalCleanup();
      await release(gate);
      const view = await wait({ kind: 'selectionIdle' });
      invalidated(view, receipt.invalidatedTaskIds);
      assert.equal(view.tasks.find((task) => task.id === otherTask)?.status, 'active');
      const content = await selection(
        { kind: 'content', target: { kind: 'task', taskId: otherTask }, cursor: null, limit: 1 },
        false,
      );
      assert.equal(content.value.page.snapshotId, otherSnapshot);
      await workspace({ kind: 'open', path: 'bitmap-rle.psd' });
      const reopened = await waitCore({ kind: 'documentCount', count: 2 });
      document = reopened.documents.find((entry) => entry.id !== other.id);
    }
    report.phase = 'cleanup-late-preview';
    await workspace({ kind: 'activate', documentId: document.id });
    await waitCore({
      kind: 'frontendImageReady',
      documentId: document.id,
      revision: document.revision,
    });
    const gate = await arm('previewPublication');
    const image = tool(
      'dev_preview',
      args({ protocolVersion: 1, documentId: document.id, revision: document.revision }),
      true,
    );
    await reached(gate);
    await externalCleanup();
    report.heldResources = (await core()).resources;
    assert(BigInt(report.heldResources.outputBytes) > 0n);
    await release(gate);
    const rejected = await image;
    assert.equal(rejected.status, 'error');
    await selection({ kind: 'releaseTask', taskId: otherTask });
    await workspace({ kind: 'close', documentId: other.id });
    report.releasedResources = (await waitCore({ kind: 'resourcesReleased' })).resources;
    report.checks.push(
      '列表／创建／内容迟到不复活，B 任务读取保留，PNG 在途引用与清理后发布拒绝，最后引用账本归零',
    );
  }
}
