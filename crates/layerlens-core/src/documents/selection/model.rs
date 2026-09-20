//! 选区和任务的核心类型；不是 IPC／MCP DTO。快照只保存范围及共享修订，不复制全文或像素。

use std::{collections::hash_map::RandomState, hash::BuildHasher, sync::Arc, time::SystemTime};

use serde::Serialize;

use crate::psd::{Capability, ColorProfileInfo, LayerId};

use super::{SelectionError, budget::Permit};
use crate::documents::{DocumentError, DocumentId, DocumentLease, RevisionId};

/// 核心实例标识；只用于区分会话，不是认证凭据。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct SessionId(String);

impl SessionId {
    pub(super) fn new() -> Self {
        // 使用标准库随机种子的独立散列器，不新增依赖，也不把时间／进程号当唯一性保证。
        let a = RandomState::new().hash_one(0_u8);
        let b = RandomState::new().hash_one(1_u8);
        Self(format!("{a:016x}{b:016x}"))
    }

    /// 可供后续边界契约传输的会话标识，不跨实例排序。
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::str::FromStr for SessionId {
    type Err = SelectionError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.len() != 32
            || !value
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        {
            return Err(SelectionError::InvalidInput(
                "会话 ID 应为 32 位小写十六进制",
            ));
        }
        Ok(Self(value.to_owned()))
    }
}

pub(super) fn decimal(value: &str, zero: bool) -> Result<u64, SelectionError> {
    if value.is_empty()
        || value.len() > 20
        || !value.bytes().all(|c| c.is_ascii_digit())
        || (value.len() > 1 && value.starts_with('0'))
    {
        return Err(SelectionError::InvalidInput("标识符应为规范十进制字符串"));
    }
    let parsed = value
        .parse::<u64>()
        .map_err(|_| SelectionError::InvalidInput("标识符超出 u64 范围"))?;
    if parsed == 0 && !zero {
        return Err(SelectionError::InvalidInput("标识符不能为零"));
    }
    Ok(parsed)
}

macro_rules! identifier {
    ($name:ident, $doc:literal, $zero:expr) => {
        #[doc = $doc]
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
        pub struct $name(pub(super) u64);
        impl $name {
            /// 本会话内的数值，必须与会话标识一起解释。
            pub fn get(self) -> u64 {
                self.0
            }
        }
        impl Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.collect_str(&self.0)
            }
        }
        impl std::str::FromStr for $name {
            type Err = SelectionError;
            fn from_str(value: &str) -> Result<Self, Self::Err> {
                decimal(value, $zero).map(Self)
            }
        }
    };
}
identifier!(
    SelectionRevision,
    "活动文档／修订／选区的全局单调版本，与快照 ID 无数值对应关系。",
    true
);
identifier!(SnapshotId, "一份不可变选区的实例内标识。", false);
identifier!(
    TaskId,
    "设计范围绑定标识；不代表 Agent 或导出作业执行状态。",
    false
);

/// 外部文档引用经过规范化解析后才能用于当前修订查找。
#[derive(Debug, Clone, Copy)]
pub struct DocumentReference {
    pub document_id: DocumentId,
    pub revision_id: RevisionId,
}
impl DocumentReference {
    /// 只接受非零规范十进制；引用是否仍存活须由服务校验。
    pub fn parse(document: &str, revision: &str) -> Result<Self, SelectionError> {
        Ok(Self {
            document_id: DocumentId(decimal(document, false)?),
            revision_id: RevisionId(decimal(revision, false)?),
        })
    }
}

/// 文档像素坐标，保留小数；核心提交时验证有限、正面积且位于画布内。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
pub struct SelectionBounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// 带文档及修订的图层引用，不以同名图层或另一文档的局部 ID 代替。
#[derive(Debug, Clone, Copy)]
pub struct LayerReference {
    pub document_id: DocumentId,
    pub revision_id: RevisionId,
    pub layer_id: LayerId,
}

impl DocumentLease {
    /// 为本修订中的图层建立显式引用；不存在时拒绝。
    pub fn selection_layer(&self, layer_id: LayerId) -> Result<LayerReference, DocumentError> {
        if !self.info().layers.iter().any(|layer| layer.id == layer_id) {
            return Err(DocumentError::LayerNotFound(layer_id));
        }
        Ok(LayerReference {
            document_id: self.document_id(),
            revision_id: self.revision_id(),
            layer_id,
        })
    }
}

/// 首版只接受同一修订的多图层或单个矩形；空列表不是隐式清空操作。
#[derive(Debug, Clone)]
pub enum SelectionInput {
    Layers(Vec<LayerReference>),
    Region(SelectionBounds),
}

/// 去除重复操作后的完整选择项；显式组与其后代仍分别保留。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SelectionItem {
    Layer { layer_id: LayerId },
    Region { bounds: SelectionBounds },
}

/// 固定的范围与能力限制；不是可通过续页消除的截断。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub enum SelectionWarning {
    GeometricBoundsOnly,
    NoReferenceBounds,
    PixelDependenciesUnresolved,
}

/// 有效目标与仅供解释层级的结构记录；结构记录不增加授权。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub enum LayerRole {
    Target,
    Structure,
}

/// 图层用于命中的完整边界是否全部位于所选矩形中。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub enum IntersectionKind {
    Contained,
    Partial,
}

/// 交集基于当前适配器的几何边界；绝不覆盖图层原始 bounds。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, ts_rs::TS)]
pub struct LayerIntersection {
    pub kind: IntersectionKind,
    pub bounds: SelectionBounds,
}

#[derive(Debug, Clone)]
pub(super) struct ContentRecord {
    pub layer_id: LayerId,
    pub stack_index: u32,
    pub role: LayerRole,
    pub intersection: Option<LayerIntersection>,
}

/// 只读快照的完整授权范围。结构引用不授予额外目标／像素访问。
#[derive(Debug, Clone)]
pub struct SelectionScope {
    pub items: Vec<SelectionItem>,
    pub target_layer_ids: Vec<LayerId>,
    pub selected_group_ids: Vec<LayerId>,
    pub group_refs: Vec<LayerId>,
    /// 当前适配器不能可靠解析额外像素依赖；空集合不代表依赖已验证可用。
    pub dependency_layer_ids: Vec<LayerId>,
    pub reference_bounds: Option<SelectionBounds>,
    pub warnings: Vec<SelectionWarning>,
    pub(super) records: Vec<ContentRecord>,
    pub(super) text_layer_count: u32,
}

impl SelectionScope {
    pub(super) fn charged_bytes(&self) -> u64 {
        (std::mem::size_of::<Self>()
            + self.items.capacity() * std::mem::size_of::<SelectionItem>()
            + (self.target_layer_ids.capacity()
                + self.selected_group_ids.capacity()
                + self.group_refs.capacity()
                + self.dependency_layer_ids.capacity())
                * std::mem::size_of::<LayerId>()
            + self.warnings.capacity() * std::mem::size_of::<SelectionWarning>()
            + self.records.capacity() * std::mem::size_of::<ContentRecord>()) as u64
    }
}

pub(super) struct Snapshot {
    pub document_name: String,
    pub id: SnapshotId,
    pub session_id: SessionId,
    pub document: DocumentLease,
    pub scope: SelectionScope,
    pub page_bytes: u64,
    // 最后归还快照额度；源修订本身使用既有的独立账本。
    pub _permit: Permit,
}

/// 请求固定的快照引用；改选、任务释放、关闭、重载和退出不改变其数据。
#[derive(Clone)]
pub struct SnapshotLease(pub(super) Arc<Snapshot>);

/// 任务固定修订的文档级事实；不借此暴露范围外的图层或文字。
#[derive(Debug)]
pub struct SnapshotDocumentInfo<'a> {
    pub document_id: DocumentId,
    pub revision_id: RevisionId,
    pub width: u32,
    pub height: u32,
    pub bit_depth: u16,
    pub color_mode: &'a str,
    pub resolution_dpi: Option<[f64; 2]>,
    pub color_profile: Option<&'a ColorProfileInfo>,
    pub preview: &'a Capability,
    pub text: &'a Capability,
}

impl std::fmt::Debug for SnapshotLease {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SnapshotLease")
            .field("id", &self.id())
            .finish_non_exhaustive()
    }
}

impl SnapshotLease {
    /// 有界范围摘要；不含完整选择项、文字、像素或源引用，可保留在终态任务中。
    pub fn brief(&self) -> SnapshotBrief {
        SnapshotBrief {
            snapshot_id: self.id(),
            document_id: self.document_id(),
            revision_id: self.revision_id(),
            document_name: self.0.document_name.clone(),
            target_count: self.scope().target_layer_ids.len() as u32,
            content_count: self.scope().records.len() as u32,
            text_layer_count: self.scope().text_layer_count,
        }
    }
    /// 本会话中的快照 ID。
    pub fn id(&self) -> SnapshotId {
        self.0.id
    }
    /// 创建此快照的核心会话。
    pub fn session_id(&self) -> &SessionId {
        &self.0.session_id
    }
    /// 固定的源文档 ID；关闭再打开不改变此值。
    pub fn document_id(&self) -> DocumentId {
        self.0.document.document_id()
    }
    /// 固定的源修订 ID；重载不改变此值。
    pub fn revision_id(&self) -> RevisionId {
        self.0.document.revision_id()
    }
    /// 不可变范围；不能由调用方修改或扩大。
    pub fn scope(&self) -> &SelectionScope {
        &self.0.scope
    }
    /// 固定修订的源指纹，不读取当前路径。
    pub fn source_sha256(&self) -> &str {
        self.0.document.source_sha256()
    }
    /// 取得任务自己的文档信息，不要求标签仍然打开，也不读取新修订。
    pub fn document_info(&self) -> SnapshotDocumentInfo<'_> {
        let info = self.0.document.info();
        SnapshotDocumentInfo {
            document_id: self.document_id(),
            revision_id: self.revision_id(),
            width: info.width,
            height: info.height,
            bit_depth: info.bit_depth,
            color_mode: &info.color_mode,
            resolution_dpi: info.resolution_dpi,
            color_profile: info.color_profile.as_ref(),
            preview: &info.preview,
            text: &info.text,
        }
    }
}

/// 同一短锁中捕获的活动摘要和强引用；内容须从此引用而不是再次采样取得。
#[derive(Debug, Clone)]
pub struct UserSelection {
    pub session_id: SessionId,
    pub revision: SelectionRevision,
    pub document_id: Option<DocumentId>,
    pub document_revision: Option<RevisionId>,
    pub snapshot: Option<SnapshotLease>,
}

/// 锁外计算的待提交结果，携带提交前置版本。不能直接作为任务授权。
pub struct PreparedSelection {
    pub(super) document_name: String,
    pub(super) document: DocumentLease,
    pub(super) expected_revision: SelectionRevision,
    pub(super) scope: SelectionScope,
}

/// 当前核心支持绑定与释放；主动全部清理／失效状态留待后续显式入口。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    Active,
    Released,
}

/// 有界任务记录；释放后保留小型终态和请求去重信息，不保留源修订。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesignTask {
    pub scope: SnapshotBrief,
    pub id: TaskId,
    pub snapshot_id: SnapshotId,
    pub name: String,
    pub created_at: SystemTime,
    pub status: TaskStatus,
}

/// 固定修订与范围计数；显示名称最多 256 UTF-8 字节，可能截短，不用于唯一定位。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotBrief {
    pub snapshot_id: SnapshotId,
    pub document_id: DocumentId,
    pub revision_id: RevisionId,
    pub document_name: String,
    pub target_count: u32,
    pub content_count: u32,
    pub text_layer_count: u32,
}

/// 与文档列表原子采样的轻量选择投影，不持有快照／修订强引用。
#[derive(Debug, Clone)]
pub struct SelectionSummary {
    pub session_id: SessionId,
    pub revision: SelectionRevision,
    pub document_id: Option<DocumentId>,
    pub document_revision: Option<RevisionId>,
    pub snapshot: Option<SnapshotBrief>,
    pub items: Vec<SelectionItem>,
}

/// 按单调任务 ID 续读；终态不移除，不因释放造成分页偏移。
#[derive(Debug)]
pub struct TaskPage {
    pub tasks: Vec<DesignTask>,
    pub next_after: Option<TaskId>,
    pub total: usize,
    pub capacity: usize,
}

/// 新增选区额度，不改变 M1 解析／预览默认值；内存数字仅是范围对象计费。
#[derive(Debug, Clone, Copy)]
pub struct SelectionConfig {
    /// 提交前原始图层引用数量上限（含重复）；默认 4096。
    pub max_items: usize,
    /// 目标与结构记录合计上限；默认 4096。
    pub max_scope_layers: usize,
    /// 包含当前选区、缓存、任务及外部引用的快照上限；默认 64。
    pub max_live_snapshots: u64,
    /// 存活范围对象的合计计费上限；默认 8 MiB，不含源数据与进程开销。
    pub max_scope_bytes: u64,
    /// 旧选择缓存条目上限；默认 16，允许为零。
    pub max_cached_snapshots: usize,
    /// 默认 128，包含已释放记录；满载后拒绝新请求，不淘汰幂等记录。
    pub max_task_records: usize,
    /// 完整内容页 JSON 上限；默认 256 KiB，可配 1–256 KiB。
    pub content_page_bytes: u64,
}

impl Default for SelectionConfig {
    fn default() -> Self {
        Self {
            max_items: 4096,
            max_scope_layers: 4096,
            max_live_snapshots: 64,
            max_scope_bytes: 8 * 1024 * 1024,
            max_cached_snapshots: 16,
            max_task_records: 128,
            content_page_bytes: 256 * 1024,
        }
    }
}

impl SelectionConfig {
    pub(in crate::documents) fn validate(self) -> Result<(), DocumentError> {
        if self.max_items == 0
            || self.max_scope_layers == 0
            || self.max_live_snapshots == 0
            || self.max_scope_bytes == 0
            || self.max_task_records == 0
            || !(1024..=256 * 1024).contains(&self.content_page_bytes)
        {
            return Err(DocumentError::InvalidConfiguration(
                "选区额度必须非零，内容页应为 1–256 KiB",
            ));
        }
        Ok(())
    }
}

/// 所有强引用中的快照范围额度；缓存／任务共享引用不会重复计费。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SelectionAccounting {
    pub live_snapshots: u64,
    pub scope_bytes: u64,
}

/// 快照分页游标，只能从页结果取得；绑定会话、快照及原文偏移。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentCursor {
    pub(super) session_id: SessionId,
    pub(super) snapshot_id: SnapshotId,
    pub(super) record: u32,
    pub(super) text_start: u32,
}
impl ContentCursor {
    /// 从已校验边界字段恢复游标；content_page 继续校验快照、记录及 UTF-16 偏移。
    pub fn from_parts(
        session_id: SessionId,
        snapshot_id: SnapshotId,
        record: u32,
        text_start: u32,
    ) -> Self {
        Self {
            session_id,
            snapshot_id,
            record,
            text_start,
        }
    }
    /// 借用组成字段供协议适配，不开放内部可变访问。
    pub fn parts(&self) -> (&SessionId, SnapshotId, u32, u32) {
        (
            &self.session_id,
            self.snapshot_id,
            self.record,
            self.text_start,
        )
    }
}

pub(super) fn limit(resource: &'static str, limit: u64) -> SelectionError {
    DocumentError::ResourceLimit { resource, limit }.into()
}
