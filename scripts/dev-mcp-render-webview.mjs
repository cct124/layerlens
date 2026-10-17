// Konva 真实交互通过开发 MCP 验收；不调用前端方法或写组件状态。
import assert from 'node:assert/strict';
export async function exerciseRenderLab({
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
  document,
}) {
  report.phase = 'render-lab';
  report.renderSamples = [];
  const page = (
    await tool(
      'dev_layers',
      args({
        protocolVersion: 1,
        documentId: document.id,
        revision: document.revision,
        query: { kind: 'list', offset: 0, limit: 64 },
      }),
    )
  ).value.result.page;
  const id = (name) => {
    const found = page.layers.find((l) => l.name === name);
    assert(found, '缺少样本图层 ' + name);
    return found.id;
  };
  assert.equal(page.total, 11);
  const fixed = (await core()).selection;
  report.initialRender = (await state()).renderLab;
  report.measurementLimits = [
    '首次自动初始化单独保留；后续重建为热启动，不能混为同一指标。',
    '绘制为 API 提交和引擎就绪时间，不包含物理呈现、RSS 或独立 Photoshop 保真结论。',
  ];
  const renderWait = (engine, generation = '0', phase = 'ready') =>
    wait({ kind: 'renderer', engine, phase, generationAtLeast: generation });
  const selectionWait = (layer, autoSelect = true, ctrl = false) =>
    wait({ kind: 'renderInteraction', selectedLayerId: layer, autoSelect, ctrl });
  const canvasClick = async (x, y) => {
    const p = point(await state(), x, y);
    return action({
      kind: 'canvasGesture',
      steps: [
        { kind: 'move', ...p },
        { kind: 'down', button: 'left' },
        { kind: 'up', button: 'left' },
      ],
      hold: false,
    });
  };
  async function mutateRender(target, engine) {
    const v = await state();
    await click(target);
    return renderWait(engine, String(BigInt(v.renderLab.generation) + 1n));
  }
  const engine = 'konva';
  let v = await mutateRender('render:rebuild', engine);
  assert.equal(v.renderLab.fontReady, true);
  assert.match(v.renderLab.sourceSha256, /^[a-f0-9]{64}$/);
  assert.equal(
    v.renderLab.fontSha256,
    'bfb7bb691513f12e734dc346c03a03f784912432d7e3fa8e56efcf906fe86b3d',
  );
  assert(v.renderLab.nodeCount >= 10);
  assert(v.renderLab.resourceCount > 0);
  for (const percent of [100, 200, 400]) {
    v = await mutateRender('render:' + percent, engine);
    assert.equal(v.canvas.zoom, percent / 100);
    report.renderSamples.push({
      engine,
      percent,
      view: v,
      measurement: 'engine draw submission; not physical presentation or RSS',
    });
    await screenshot('render-' + engine + '-' + percent + '.png');
  }
  await mutateRender('render:reset', engine);
  const hoverPoint = point(await state(), 55, 200);
  const hover = await action({ kind: 'movePointer', ...hoverPoint });
  const hovered = await wait({
    kind: 'canvasIdle',
    pointerEventsAtLeast: String(BigInt(hover.value.before.input.pointerEvents) + 1n),
  });
  assert.equal(hovered.renderLab.hoverLayerId, id('alpha-window'));
  assert.equal(hovered.renderLab.selectedLayerId, hover.value.before.renderLab.selectedLayerId);
  await canvasClick(55, 200);
  await selectionWait(id('alpha-window'));
  await canvasClick(85, 220);
  await selectionWait(id('blue-card'));
  await canvasClick(140, 230);
  v = await selectionWait(id('overlap-card'));
  assert(v.renderLab.candidateIds.includes(id('blue-card')));
  await click('render:next');
  await selectionWait(id('blue-card'));
  await click('render:auto');
  await selectionWait(id('blue-card'), false);
  assert.equal((await state()).renderLab.hoverLayerId, null);
  const press = await canvasClick(140, 230);
  await wait({
    kind: 'canvasIdle',
    pointerEventsAtLeast: String(BigInt(press.value.before.input.pointerEvents) + 2n),
  });
  assert.equal((await state()).renderLab.selectedLayerId, id('blue-card'));
  await key('control', 'down');
  await selectionWait(id('blue-card'), false, true);
  await canvasClick(140, 230);
  await selectionWait(id('overlap-card'), false, true);
  await key('control', 'up');
  await selectionWait(id('overlap-card'), false, false);
  await click('render:auto');
  await selectionWait(id('overlap-card'));
  const before = await state(),
    p = point(before, 220, 240);
  await action({
    kind: 'canvasGesture',
    steps: [
      { kind: 'move', ...p },
      { kind: 'down', button: 'middle' },
      { kind: 'move', x: p.x + 23, y: p.y - 17 },
      { kind: 'up', button: 'middle' },
    ],
    hold: false,
  });
  v = await renderWait(engine, String(BigInt(before.renderLab.generation) + 1n));
  assert(Math.abs(v.canvas.panX - before.canvas.panX - 23) < 0.1);
  assert(Math.abs(v.canvas.panY - before.canvas.panY + 17) < 0.1);
  assert.equal(
    (await core()).selection.selectionRevision,
    fixed.selectionRevision,
    '检查选择不能改变任务授权',
  );
  await mutateRender('render:reset', engine);
  report.checks.push(
    engine + ': 100/200/400%、源字体同一、alpha 命中、重叠候选、自动/Ctrl 选择、核心同步与中键平移',
  );
  await click('render:release');
  const released = await renderWait(engine, '0', 'released');
  assert.equal(released.renderLab.resourceCount, 0);
  assert.equal(released.renderLab.fontReady, false);
  await click('render:rebuild');
  await renderWait(engine, String(BigInt(released.renderLab.generation) + 1n));
  await workspace({ kind: 'close', documentId: document.id });
  await waitCore({ kind: 'documentCount', count: 0 });
  const closed = await renderWait(engine, '0', 'released');
  assert.equal(closed.renderLab.resourceCount, 0);
  assert.equal(closed.renderLab.sourceSha256, null);
  await workspace({ kind: 'open', path: 'render-simple.psd' });
  const reopened = await waitCore({ kind: 'documentCount', count: 1 });
  assert.notEqual(reopened.documents[0].id, document.id);
  await wait({
    kind: 'imageReady',
    documentId: reopened.documents[0].id,
    revision: reopened.documents[0].revision,
  });
  await screenshot('final-state.png');
  report.checks.push('显式释放、同窗口重建、关闭/重开文档和新核心身份；对象计数不代表 RSS');
}
