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

export type WorkspaceAction = { "kind": "snapshot", } | { "kind": "open", path: string, } | { "kind": "activate", documentId: string, } | { "kind": "close", documentId: string, } | { "kind": "reload", documentId: string, } | { "kind": "retryPreview", } | { "kind": "cancel", jobId: string, } | { "kind": "dismissNotice", noticeId: string, };

export type WorkspaceRequest = { protocolVersion: number, action: WorkspaceAction, };

export type PreviewRequest = { protocolVersion: number, documentId: string, revision: string, };

export type WorkspaceErrorCode = "PROTOCOL_MISMATCH" | "INVALID_INPUT" | "NOT_FOUND" | "BUSY" | "RESOURCE_LIMIT" | "OPEN_FAILED" | "PREVIEW_FAILED" | "STALE_PREVIEW" | "SHUTTING_DOWN" | "INTERNAL";

export type WorkspaceError = { code: WorkspaceErrorCode, message: string, };

export type WorkspaceDocument = { id: string, revision: string, name: string, path: string, width: number, height: number, layerCount: number, colorMode: string, bitDepth: number, previewNote: string, };

export type WorkspaceJobPhase = "queued" | "running" | "cancelling" | "finishing";

export type WorkspaceJob = { id: string, label: string, phase: WorkspaceJobPhase, };

export type WorkspacePreviewState = { "phase": "pending", jobId: string | null, status: WorkspaceJobPhase, } | { "phase": "ready", cacheHit: boolean, } | { "phase": "failed", error: WorkspaceError, } | { "phase": "cancelled" };

export type WorkspacePreview = { documentId: string, revision: string, state: WorkspacePreviewState, };

export type WorkspaceNotice = { id: string, message: string, };

export type WorkspaceResources = { sourceBytes: string, decodedBytes: string, outputBytes: string, cacheBytes: string, };

export type WorkspaceSnapshot = { protocolVersion: number, sequence: string, documents: Array<WorkspaceDocument>, activeDocumentId: string | null, jobs: Array<WorkspaceJob>, preview: WorkspacePreview | null, notices: Array<WorkspaceNotice>, resources: WorkspaceResources, shuttingDown: boolean, };
