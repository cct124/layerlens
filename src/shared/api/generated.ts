// 此文件由 Rust DTO 自动生成，请勿手工编辑。
// 更新：cargo run -p layerlens-core --example export_bindings

export const IPC_PROTOCOL_VERSION = 1;

export type AppInfoRequest = {
/**
 * 调用方使用的 IPC 契约版本。
 */
protocolVersion: number, };

export type AppInfo = {
/**
 * 面向用户的应用名称。
 */
appName: string,
/**
 * 当前应用版本，与桌面 crate 共用 workspace 版本。
 */
appVersion: string,
/**
 * 本次响应采用的 IPC 契约版本。
 */
protocolVersion: number, };

export type CommandErrorCode = "PROTOCOL_MISMATCH";

export type CommandError = {
/**
 * 稳定的机器可读错误代码。
 */
code: CommandErrorCode,
/**
 * 包含原因及恢复提示的用户消息。
 */
message: string, };

export type WorkspaceAction = { "kind": "snapshot", } | { "kind": "open", path: string, } | { "kind": "activate", documentId: string, } | { "kind": "close", documentId: string, } | { "kind": "reload", documentId: string, } | { "kind": "selectLayer", documentId: string, revision: string, layerId: number | null, } | { "kind": "retryPreview", } | { "kind": "cancel", jobId: string, } | { "kind": "dismissNotice", noticeId: string, };

export type WorkspaceRequest = { protocolVersion: number, action: WorkspaceAction, };

export type PreviewRequest = { protocolVersion: number, documentId: string, revision: string, };

export type WorkspaceErrorCode = "PROTOCOL_MISMATCH" | "INVALID_INPUT" | "NOT_FOUND" | "BUSY" | "RESOURCE_LIMIT" | "OPEN_FAILED" | "PREVIEW_FAILED" | "STALE_PREVIEW" | "STALE_REVISION" | "FOREIGN_SESSION" | "STALE_SELECTION" | "INACTIVE_DOCUMENT" | "SNAPSHOT_EXPIRED" | "EMPTY_TARGETS" | "TASK_RELEASED" | "REQUEST_CONFLICT" | "SHUTTING_DOWN" | "INTERNAL";

export type WorkspaceError = { code: WorkspaceErrorCode, message: string, };

export type WorkspaceDocument = { id: string, revision: string, name: string, path: string, width: number, height: number, layerCount: number, colorMode: string, bitDepth: number, previewNote: string, selectedLayerId: number | null, };

export type WorkspaceJobPhase = "queued" | "running" | "cancelling" | "finishing";

export type WorkspaceJob = { id: string, label: string, phase: WorkspaceJobPhase, };

export type WorkspacePreviewState = { "phase": "pending", jobId: string | null, status: WorkspaceJobPhase, } | { "phase": "ready", cacheHit: boolean, } | { "phase": "failed", error: WorkspaceError, } | { "phase": "cancelled" };

export type WorkspacePreview = { documentId: string, revision: string, state: WorkspacePreviewState, };

export type WorkspaceNotice = { id: string, message: string, };

export type WorkspaceResources = { sourceBytes: string, decodedBytes: string, outputBytes: string, cacheBytes: string, };

export type WorkspaceSnapshot = { selection: SelectionSummaryDto, protocolVersion: number, sequence: string, documents: Array<WorkspaceDocument>, activeDocumentId: string | null, jobs: Array<WorkspaceJob>, preview: WorkspacePreview | null, notices: Array<WorkspaceNotice>, resources: WorkspaceResources, shuttingDown: boolean, };

export type LayerId = number;

export type Bounds = { x: number, y: number, width: number, height: number, };

export type LayerKind = "group" | "bitmap" | "text" | "shape" | "smartObject" | "adjustment" | "unknown";

export type ExportBlocker = "unsupportedLayerKind" | "hidden" | "globalMask" | "ancestorVisualDependency" | "layerVisualDependency" | "missingRgbChannels" | "emptyBitmap";

export type Support = "supported" | "partial" | "unsupported";

export type Capability = { status: Support, reason: string, };

export type TextIndexMapping = "exact" | "trailingParagraphTerminator";

export type TextProperty<T> = { value: T, source: string, };

export type TextUnit = "unverifiedEngine";

export type TextMetric = { value: number, unit: TextUnit, };

export type TextFont = { index: number, name: string, family: string | null, style: string | null, };

export type TextColor = { red: number, green: number, blue: number, alpha: number, };

export type CharacterStyle = { font: TextProperty<TextFont> | null, fontSize: TextProperty<TextMetric> | null, fillColor: TextProperty<TextColor> | null, fillEnabled: TextProperty<boolean> | null, strokeEnabled: TextProperty<boolean> | null,
/**
 * Photoshop 的显式 FauxBold；不转换成 CSS font-weight。
 */
fauxBold: TextProperty<boolean> | null, fauxItalic: TextProperty<boolean> | null, autoLeading: TextProperty<boolean> | null, leading: TextProperty<TextMetric> | null, tracking: TextProperty<TextMetric> | null, baselineShift: TextProperty<TextMetric> | null, horizontalScale: TextProperty<number> | null, verticalScale: TextProperty<number> | null, };

export type TextStyleRun = { start: number, end: number, style: CharacterStyle, };

export type ParagraphAlignment = "left" | "right" | "center" | "justifyLeft" | "justifyRight" | "justifyCenter" | "justifyAll";

export type ParagraphStyle = { alignment: TextProperty<ParagraphAlignment> | null, autoLeading: TextProperty<number> | null, firstLineIndent: TextProperty<TextMetric> | null, startIndent: TextProperty<TextMetric> | null, endIndent: TextProperty<TextMetric> | null, spaceBefore: TextProperty<TextMetric> | null, spaceAfter: TextProperty<TextMetric> | null, };

export type ParagraphStyleRun = { start: number, end: number, style: ParagraphStyle, };

export type TextDiagnosticCode = "missing" | "invalid" | "unsupported" | "unverified" | "textMismatch" | "invalidRange";

export type TextDiagnostic = { code: TextDiagnosticCode, path: string, message: string, };

export type LayerSummary = { id: LayerId, parentId: LayerId | null, name: string, nameTruncated: boolean, kind: LayerKind, bounds: Bounds, visible: boolean, effectiveVisible: boolean, opacity: number, };

export type LayerPage = { offset: number, total: number, nextOffset: number | null, layers: Array<LayerSummary>, };

export type TextSlice = { text: string, start: number, end: number, totalLength: number, nextStart: number | null, transform: [number, number, number, number, number, number] | null, styles: Capability, indexMapping: TextIndexMapping | null, styleRuns: Array<TextStyleRun> | null, paragraphRuns: Array<ParagraphStyleRun> | null, diagnostics: Array<TextDiagnostic>, diagnosticsTruncated: boolean, };

export type LayerDetails = { layer: LayerSummary, export: Capability, exportBlockers: Array<ExportBlocker>, diagnostics: Array<string>, diagnosticsTruncated: boolean, text: TextSlice | null, };

export type LayerQuery = { "kind": "list", offset: number, limit: number, } | { "kind": "details", layerId: number, textStart: number, };

export type LayerRequest = { protocolVersion: number, documentId: string, revision: string, query: LayerQuery, };

export type LayerResult = { "kind": "list", page: LayerPage, } | { "kind": "details", details: LayerDetails, };

export type LayerResponse = { documentId: string, revision: string, result: LayerResult, };

export type SelectionBounds = { x: number, y: number, width: number, height: number, };

export type LayerIntersection = { kind: IntersectionKind, bounds: SelectionBounds, };

export type IntersectionKind = "contained" | "partial";

export type LayerRole = "target" | "structure";

export type SelectionWarning = "geometricBoundsOnly" | "noReferenceBounds" | "pixelDependenciesUnresolved";

export type ContentLayer = { stackIndex: number, role: LayerRole, intersection: LayerIntersection | null, details: LayerDetails, };

export type ScopeDto = { snapshotId: string, documentId: string, documentRevision: string, documentName: string, targetCount: number, contentCount: number, textLayerCount: number, };

export type SelectionSummaryDto = { sessionId: string, selectionRevision: string, documentId: string | null, documentRevision: string | null, scope: ScopeDto | null, layerIds: Array<number>, region: SelectionBounds | null, };

export type TaskStatusDto = "active" | "released";

export type TaskDto = { id: string, name: string, status: TaskStatusDto, scope: ScopeDto, };

export type TaskPageDto = { sessionId: string, tasks: Array<TaskDto>, nextAfter: string | null, total: number, capacity: number, };

export type SelectionCursorDto = { sessionId: string, snapshotId: string, record: number, textStart: number, };

export type SelectionContentDto = { sessionId: string, snapshotId: string, documentId: string, documentRevision: string, layerCount: number, textLayerCount: number, layers: Array<ContentLayer>, warnings: Array<SelectionWarning>, truncated: boolean, nextCursor: SelectionCursorDto | null, };

export type SelectionTarget = { "kind": "snapshot", snapshotId: string, } | { "kind": "task", taskId: string, };

export type SelectionOperation = { "kind": "layers", documentId: string, documentRevision: string, expectedRevision: string, layerIds: Array<number>, } | { "kind": "region", documentId: string, documentRevision: string, expectedRevision: string, bounds: SelectionBounds, } | { "kind": "clear", documentId: string, documentRevision: string, expectedRevision: string, } | { "kind": "createTask", snapshotId: string, requestId: string, name: string, } | { "kind": "releaseTask", taskId: string, } | { "kind": "tasks", after: string | null, limit: number, } | { "kind": "content", target: SelectionTarget, cursor: SelectionCursorDto | null, limit: number, };

export type SelectionRequest = { protocolVersion: number, sessionId: string, operation: SelectionOperation, };

export type SelectionReply = { "kind": "committed", sessionId: string, selectionRevision: string, snapshotId: string | null, } | { "kind": "task", sessionId: string, task: TaskDto, } | { "kind": "tasks", page: TaskPageDto, } | { "kind": "content", target: SelectionTarget, page: SelectionContentDto, };
