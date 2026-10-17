// 由 export_dev_bindings 从 Rust 自动生成，请勿手工编辑。

export type UiRect = { x: number, y: number, width: number, height: number, };

export type UiDocument = { id: string, revision: string, };

export type UiSnapTarget = { position: number, layerId: number | null, };

export type UiCanvas = { viewport: UiRect, image: UiRect | null, documentWidth: number, documentHeight: number, imageDecoded: boolean, paintPrepared: boolean, imageError: string | null, zoom: number, panX: number, panY: number, panning: boolean, pointerCaptured: boolean, draft: UiRect | null, alt: boolean, snapGuides: number, snapX: UiSnapTarget | null, snapY: UiSnapTarget | null, };

export type UiTask = { id: string, status: string, snapshotId: string, };

export type UiCleanup = { opened: boolean, phase: string | null, planId: string | null, taskIds: Array<string>, syncing: boolean, syncError: boolean, pending: boolean, renderedTasks: number, scrollable: boolean, scrolledToEnd: boolean, focusInside: boolean, };

export type UiContent = { taskId: string | null, snapshotId: string, ready: boolean, };

export type UiInputEvidence = { pointerEvents: string, keyEvents: string, lastKey: string | null, };

export type UiWindowControl = { target: string, label: string, rect: UiRect, disabled: boolean, };

export type UiWindow = { focused: boolean, focusEvents: string, blurEvents: string, width: number, height: number, devicePixelRatio: number, titlebar: UiRect | null, controls: Array<UiWindowControl>, };

export type UiRenderLab = { engine: string, phase: string, generation: string, sourceSha256: string | null, fontSha256: string | null, fontReady: boolean, nodeCount: number, resourceCount: number, selectedLayerId: number | null, candidateIds: Array<number>, hoverLayerId: number | null, autoSelect: boolean, ctrl: boolean, initMs: number, renderMs: number, sceneReadMs: number, assetsMs: number, firstReadyMs: number, textMode: string, nativeTextLayers: number, sharedTextLayers: number, unsupportedLayers: number, profile: string, sourceLayers: number, drawnLayerIds: Array<number>, rasterLayerIds: Array<number>, textRasterLayerIds: Array<number>, degradedLayerIds: Array<number>, unsupportedLayerIds: Array<number>, encodedBytes: number, alphaBytes: number, selectionMs: number | null, hitMs: number | null, samples: Array<UiRenderSample>, backend: string, error: string | null, limitations: Array<string>, };

export type UiRenderSample = { generation: string, operation: string, submitMs: number, readyMs: number, readbackMs: number, synchronized: boolean, frameIntervalMs: number | null, inputToReadyMs: number | null, };

export type UiObservation = { renderLab: UiRenderLab | null, instanceId: string, frontendId: string,
/**
 * 前端挂载时 timeOrigin + now 的微秒整数，用于丢弃旧挂载或旧页面迟到报告。
 */
documentEpoch: string, viewRevision: string, coreSequence: string | null, sessionId: string | null, activeDocument: UiDocument | null, selectionRevision: string | null, snapshotId: string | null, region: UiRect | null, tool: string, panel: string, menu: string | null, focus: string | null, cleanup: UiCleanup, canvas: UiCanvas | null, tasks: Array<UiTask>, selectionRunning: boolean, content: UiContent | null, input: UiInputEvidence, window: UiWindow, };

