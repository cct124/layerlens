//! 保存时合成图的请求结果与预算契约；不生成 CSS 或重新合成图层。

use super::super::{
    DocumentError, DocumentId, JobStatus, PreviewJobId, RevisionId, model::JobSignal,
};
use super::budget::OutputPermit;
use crate::psd::{Capability, PngTimings};
use std::{fmt, sync::Arc, time::Duration};

/// 预览准入上限；解码计量沿用解析器预算，不包含分配器及所有编解码器临时内存。
#[derive(Debug, Clone, Copy)]
pub struct PreviewConfig {
    /// 服务级解码预留上限；每次执行先预留完整的单文件解码额度。
    pub max_decoded_bytes: u64,
    /// 单张 PNG 的有效字节和分配容量上限；编码时检查，超限不发布部分结果。
    pub max_png_bytes: usize,
    /// 缓存、外部引用及在途输出共用；同一输出不重复计费。
    pub max_output_bytes: u64,
    /// 缓存为输出总量的子集；任一缓存上限为零即可禁用缓存。
    pub max_cache_bytes: u64,
    pub max_cache_entries: usize,
}

impl Default for PreviewConfig {
    fn default() -> Self {
        Self {
            max_decoded_bytes: 128 * 1024 * 1024,
            max_png_bytes: 64 * 1024 * 1024,
            max_output_bytes: 128 * 1024 * 1024,
            max_cache_bytes: 64 * 1024 * 1024,
            max_cache_entries: 16,
        }
    }
}
impl PreviewConfig {
    pub(crate) fn validate(self, decode: u64) -> Result<(), DocumentError> {
        if self.max_decoded_bytes < decode
            || self.max_png_bytes == 0
            || self.max_output_bytes < self.max_png_bytes as u64
        {
            return Err(DocumentError::InvalidConfiguration(
                "预览额度必须容纳一次完整解码和 PNG 输出预留",
            ));
        }
        if self.max_cache_bytes > self.max_output_bytes {
            return Err(DocumentError::InvalidConfiguration(
                "预览缓存上限不得超过输出总上限",
            ));
        }
        Ok(())
    }
}

/// 当前只支持原尺寸保存时合成图；新增渲染参数时必须扩展缓存 key。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewKind {
    SavedCompositePng,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Key {
    pub document: DocumentId,
    pub revision: RevisionId,
    pub kind: PreviewKind,
}

pub(super) struct Image {
    pub key: Key,
    pub width: u32,
    pub height: u32,
    pub capability: Capability,
    // 字段顺序保证 PNG 先释放，再归还输出额度；不持有源修订。
    pub bytes: Vec<u8>,
    pub _permit: OutputPermit,
}

/// 不可变 PNG 引用，关闭、重载和退出后仍有效；最后一个引用释放时归还容量计费。
#[derive(Clone)]
pub struct PreviewLease(pub(super) Arc<Image>);
impl PreviewLease {
    /// 生成本图像的文档标识，关闭后仍可用于关联结果。
    pub fn document_id(&self) -> DocumentId {
        self.0.key.document
    }
    /// 生成本图像的固定修订；不会随文档重载而变化。
    pub fn revision_id(&self) -> RevisionId {
        self.0.key.revision
    }
    /// 本结果采用的预览种类。
    pub fn kind(&self) -> PreviewKind {
        self.0.key.kind
    }
    /// 图像宽度，单位为原始文档像素。
    pub fn width(&self) -> u32 {
        self.0.width
    }
    /// 图像高度，单位为原始文档像素。
    pub fn height(&self) -> u32 {
        self.0.height
    }
    /// 保存时预览的支持程度及原因，包含 ICC 等既有适配限制。
    pub fn capability(&self) -> &Capability {
        &self.0.capability
    }
    /// 完整 PNG 编码数据的只读借用；不包含源 PSD，也不执行文件写入。
    pub fn png(&self) -> &[u8] {
        &self.0.bytes
    }
    /// PNG Vec 的分配容量，可能大于有效 PNG 字节数。
    pub fn charged_bytes(&self) -> u64 {
        self.0.bytes.capacity() as u64
    }
}
impl fmt::Debug for PreviewLease {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PreviewLease")
            .field("key", &self.0.key)
            .field("bytes", &self.png().len())
            .finish()
    }
}

/// 已发布结果；成功分支持有 PNG 引用，保留或克隆它会延长输出计费。
#[derive(Debug, Clone)]
pub enum PreviewOutcome {
    Ready(PreviewLease),
    Cancelled,
    Failed(Arc<DocumentError>),
}

/// 命中缓存时两个耗时为零，png_timings 为 None；不是 UI 首帧计时。
#[derive(Debug, Clone)]
pub struct PreviewCompletion {
    pub outcome: PreviewOutcome,
    pub cache_hit: bool,
    pub queued_for: Duration,
    pub executed_for: Duration,
    pub png_timings: Option<PngTimings>,
}

/// 相同修订的在途请求合并为同一作业，任一句柄取消都会取消共同工作。
/// 丢弃句柄不取消；完成后句柄会保留结果 PNG，仍计入输出预算。
#[derive(Clone)]
pub struct PreviewJob {
    pub(in super::super) id: PreviewJobId,
    pub(in super::super) signal: Arc<JobSignal<PreviewCompletion>>,
}
impl PreviewJob {
    /// 用于取消与状态关联的标识；合并请求返回相同标识。
    pub fn id(&self) -> PreviewJobId {
        self.id
    }
    /// 非阻塞取得当前状态；保留成功状态也会持有其 PNG 引用。
    pub fn status(&self) -> JobStatus<PreviewCompletion> {
        self.signal.status()
    }
    /// 阻塞等待，须由适配层放在后台执行。
    pub fn wait(&self) -> Arc<PreviewCompletion> {
        self.signal.wait()
    }
    /// 超时只返回 None，不取消后台工作。
    pub fn wait_timeout(&self, timeout: Duration) -> Option<Arc<PreviewCompletion>> {
        self.signal.wait_timeout(timeout)
    }
}

/// 服务级预览资源计量，不包含源修订、分配器及全部编解码器临时开销。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PreviewAccounting {
    /// 正在执行的作业所预留的解码额度，不代表实时像素分配量。
    pub decoded_bytes: u64,
    /// 实际存活 PNG 容量，加在途作业预留。
    pub output_bytes: u64,
}

/// 缓存状态及服务实例内累计统计；资源账本独立采样，可能同时发生引用释放。
#[derive(Debug, Default, Clone, Copy)]
pub struct PreviewSnapshot {
    pub resources: PreviewAccounting,
    pub cache_entries: usize,
    pub cache_bytes: u64,
    pub cache_hits: u64,
    pub coalesced_requests: u64,
    /// 因容量或输出准入而淘汰的条目数，不包含关闭、重载和退出的主动失效。
    pub evictions: u64,
}
