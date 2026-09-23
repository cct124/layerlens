//! 只读工作区的传输契约；不包含 Tauri 类型、解析对象或图像字节。
//! ID 和顺序号用十进制字符串传输，避免 JavaScript number 丢失 u64 精度。

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// 一次工作区操作；打开成功前由作业代表请求，不创建虚假文档。
/// 无参数操作也使用空结构体变体，使 Serde 严格拒绝额外字段。
#[derive(Debug, Clone, Deserialize, Serialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum WorkspaceAction {
    Snapshot {},
    Open {
        path: String,
    },
    Activate {
        document_id: String,
    },
    Close {
        document_id: String,
    },
    Reload {
        document_id: String,
    },
    SelectLayer {
        document_id: String,
        revision: String,
        layer_id: Option<u32>,
    },
    RetryPreview {},
    Cancel {
        job_id: String,
    },
    DismissNotice {
        notice_id: String,
    },
}

/// 每次调用校验协议版本；结构反序列化之后仍须校验 ID、路径及资源准入。
#[derive(Debug, Clone, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceRequest {
    pub protocol_version: u32,
    pub action: WorkspaceAction,
}

/// 只允许读取当前活动文档、当前修订已经完成的预览。
#[derive(Debug, Clone, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreviewRequest {
    pub protocol_version: u32,
    pub document_id: String,
    pub revision: String,
}

/// 可恢复边界错误；message 保留操作原因，前端不得将错误当作空状态。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceError {
    pub code: WorkspaceErrorCode,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, TS)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum WorkspaceErrorCode {
    ProtocolMismatch,
    InvalidInput,
    NotFound,
    Busy,
    ResourceLimit,
    OpenFailed,
    PreviewFailed,
    StalePreview,
    StaleRevision,
    ForeignSession,
    StaleSelection,
    InactiveDocument,
    SnapshotExpired,
    EmptyTargets,
    TaskReleased,
    TaskInvalidated,
    DocumentInvalidated,
    StaleCleanupPlan,
    RequestConflict,
    ShuttingDown,
    Internal,
}

/// 轻量文档信息。坐标和尺寸均为 PSD 原稿像素，不推导 CSS。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceDocument {
    pub id: String,
    pub revision: String,
    pub name: String,
    pub path: String,
    pub width: u32,
    pub height: u32,
    pub layer_count: u32,
    pub color_mode: String,
    pub bit_depth: u16,
    pub preview_note: String,
    pub selected_layer_id: Option<u32>,
}

/// 排队、计算和取消后的收尾分别展示，不将取消请求解释为计算已停止。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum WorkspaceJobPhase {
    Queued,
    Running,
    Cancelling,
    Finishing,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceJob {
    pub id: String,
    pub label: String,
    pub phase: WorkspaceJobPhase,
}

/// 预览状态固定目标修订。ready 只表示核心 PNG 就绪，不表示界面已可见。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, TS)]
#[serde(
    tag = "phase",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum WorkspacePreviewState {
    Pending {
        job_id: Option<String>,
        status: WorkspaceJobPhase,
    },
    Ready {
        cache_hit: bool,
    },
    Failed {
        error: WorkspaceError,
    },
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct WorkspacePreview {
    pub document_id: String,
    pub revision: String,
    pub state: WorkspacePreviewState,
}

/// 有界通知列表；保留最近 16 条失败／取消，确认后可移除。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceNotice {
    pub id: String,
    pub message: String,
}

/// 资源计量是准入账本，非进程 RSS；数值以字节十进制字符串表示。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceResources {
    pub source_bytes: String,
    pub decoded_bytes: String,
    pub output_bytes: String,
    pub cache_bytes: String,
}

/// 原子文档摘要及适配层作业状态。前端仅接受更大的 sequence；重连重新读取快照。
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSnapshot {
    pub selection: crate::selection_contract::SelectionSummaryDto,
    pub protocol_version: u32,
    pub sequence: String,
    pub documents: Vec<WorkspaceDocument>,
    pub active_document_id: Option<String>,
    pub jobs: Vec<WorkspaceJob>,
    pub preview: Option<WorkspacePreview>,
    pub notices: Vec<WorkspaceNotice>,
    pub resources: WorkspaceResources,
    pub shutting_down: bool,
}

/// 按需图层读取，不进入工作区通知。数量、文字偏移及目标修订由核心校验。
#[derive(Debug, Clone, Deserialize, Serialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum LayerQuery {
    List { offset: u32, limit: u32 },
    Details { layer_id: u32, text_start: u32 },
}

#[derive(Debug, Clone, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LayerRequest {
    pub protocol_version: u32,
    pub document_id: String,
    pub revision: String,
    pub query: LayerQuery,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum LayerResult {
    List {
        page: crate::documents::LayerPage,
    },
    Details {
        // 详情远大于列表页，装箱避免整个结果枚举按大变体移动。
        details: Box<crate::documents::LayerDetails>,
    },
}

/// 响应携带实际固定的目标关联，前端不能仅凭到达顺序展示属性。
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct LayerResponse {
    pub document_id: String,
    pub revision: String,
    pub result: LayerResult,
}
