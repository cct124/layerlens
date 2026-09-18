//! 核心文档、修订与共享作业状态类型；尚未作为桌面或 MCP 的线协议发布。

use std::{
    path::PathBuf,
    sync::{Arc, Condvar, Mutex, MutexGuard},
    time::Duration,
};

use crate::psd::{DocumentInfo, ParseLimits, PsdDocument};

use super::{
    DocumentError,
    budget::Permit,
    preview::{PreviewCompletion, PreviewConfig, PreviewSnapshot},
};

macro_rules! identifier {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(pub(super) u64);
        impl $name {
            /// 本服务实例内唯一的数值；不同 ID 类型不可互换。
            pub fn get(self) -> u64 {
                self.0
            }
        }
    };
}
identifier!(DocumentId, "文档查看入口的稳定标识；重载不改变它。");
identifier!(RevisionId, "一次成功解析产生的不可变修订标识。");
identifier!(OpenJobId, "打开／重载作业标识，与后续设计任务 ID 分离。");
identifier!(PreviewJobId, "保存时合成图预览作业标识。");

/// 单个后台线程的解析和预览准入配置。计数并非进程内存硬上限。
#[derive(Debug, Clone, Copy)]
pub struct DocumentServiceConfig {
    pub parse_limits: ParseLimits,
    /// 打开和预览共用的排队、执行及清理中作业总数，含等待同步计算结束的取消。
    pub max_pending_jobs: usize,
    /// 已发布、仍被引用的旧修订和在途解析的合计上限。
    pub max_live_revisions: u64,
    /// 源字节计费上限；在途解析先按单文件上限预留。
    pub max_source_bytes: u64,
    /// 声明像素计费上限；在途解析先按单文件上限预留。
    pub max_declared_pixels: u64,
    /// 解码、PNG 输出及其缓存的独立额度。
    pub preview: PreviewConfig,
}

impl Default for DocumentServiceConfig {
    fn default() -> Self {
        Self {
            parse_limits: ParseLimits::default(),
            max_pending_jobs: 4,
            max_live_revisions: 8,
            max_source_bytes: 256 * 1024 * 1024,
            max_declared_pixels: 64 * 1024 * 1024,
            preview: PreviewConfig::default(),
        }
    }
}

impl DocumentServiceConfig {
    pub(super) fn validate(self) -> Result<Self, DocumentError> {
        let limits = self.parse_limits;
        if self.max_pending_jobs == 0 || self.max_live_revisions == 0 {
            return Err(DocumentError::InvalidConfiguration(
                "作业和修订上限必须大于零",
            ));
        }
        if limits.max_file_bytes == 0
            || limits.max_total_pixels == 0
            || limits.max_layers == 0
            || limits.max_decoded_bytes == 0
        {
            return Err(DocumentError::InvalidConfiguration("单文件限制必须大于零"));
        }
        if self.max_source_bytes < limits.max_file_bytes
            || self.max_declared_pixels < limits.max_total_pixels
        {
            return Err(DocumentError::InvalidConfiguration(
                "全局额度必须容纳一次完整解析预留",
            ));
        }
        self.preview.validate(limits.max_decoded_bytes)?;
        Ok(self)
    }
}

/// 已使用或预留的额度；关闭入口不保证立即归零。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ResourceAccounting {
    pub live_revisions: u64,
    pub source_bytes: u64,
    pub declared_pixels: u64,
}

/// 不含文字／像素的大纲，用于查看状态而不复制完整文档。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentSummary {
    pub id: DocumentId,
    pub revision: RevisionId,
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
}

/// 标签顺序、活动入口与作业状态在同一次短锁访问中取得。
#[derive(Debug, Clone)]
pub struct ServiceSnapshot {
    pub documents: Vec<DocumentSummary>,
    pub active_document: Option<DocumentId>,
    pub pending_jobs: Vec<(OpenJobId, JobStatus)>,
    pub pending_previews: Vec<(PreviewJobId, JobStatus<PreviewCompletion>)>,
    pub previews: PreviewSnapshot,
    /// 独立资源账本的即时采样，后台计算／引用释放可同时更新它。
    pub resources: ResourceAccounting,
    pub shutting_down: bool,
}

pub(super) struct Revision {
    pub document_id: DocumentId,
    pub id: RevisionId,
    // 字段顺序保证解析数据先销毁，再由 Permit 归还资源额度。
    pub parsed: PsdDocument,
    pub _permit: Permit,
}

/// 固定修订的只读引用。重载、关闭和服务退出均不改变已取得的数据。
///
/// 最后一个引用释放后才归还其全局额度；仅暴露元数据，预览通过服务调度。
#[derive(Clone)]
pub struct DocumentLease(pub(super) Arc<Revision>);

impl DocumentLease {
    /// 当前修订所属文档的稳定标识。
    pub fn document_id(&self) -> DocumentId {
        self.0.document_id
    }
    /// 本引用固定的修订标识。
    pub fn revision_id(&self) -> RevisionId {
        self.0.id
    }
    /// PSD 规范化事实，保留原始单位、诊断及能力限制。
    pub fn info(&self) -> &DocumentInfo {
        self.0.parsed.info()
    }
    /// 本次解析源字节的 SHA-256，不重新读取外部文件。
    pub fn source_sha256(&self) -> &str {
        self.0.parsed.source_sha256()
    }
}

/// 已完成作业只保留小型结果，不隐式持有解析文档。
#[derive(Debug, Clone)]
pub enum OpenOutcome {
    Opened {
        document_id: DocumentId,
        revision_id: RevisionId,
        reused: bool,
    },
    Cancelled,
    Failed(Arc<DocumentError>),
}

/// 核心排队／执行计时，不包含前端可见延迟，也不代表进程内存。
#[derive(Debug, Clone)]
pub struct JobCompletion {
    pub outcome: OpenOutcome,
    pub queued_for: Duration,
    pub executed_for: Duration,
}

/// 取消接受后，运行中的同步计算仍可能占用执行名额和预算。
#[derive(Debug, Clone)]
pub enum JobStatus<C = JobCompletion> {
    Queued,
    Running,
    Cancelling,
    /// 发布决定已提交，正在锁外释放被丢弃的数据；此时不再接受取消。
    Finishing,
    Finished(Arc<C>),
}

pub(super) struct JobSignal<C = JobCompletion> {
    status: Mutex<JobStatus<C>>,
    changed: Condvar,
}

impl<C: Clone> JobSignal<C> {
    pub fn new() -> Self {
        Self {
            status: Mutex::new(JobStatus::Queued),
            changed: Condvar::new(),
        }
    }
    pub fn set(&self, status: JobStatus<C>) {
        *lock(&self.status) = status;
        self.changed.notify_all();
    }
    pub fn status(&self) -> JobStatus<C> {
        lock(&self.status).clone()
    }
    pub fn wait(&self) -> Arc<C> {
        let mut status = lock(&self.status);
        loop {
            if let JobStatus::Finished(result) = &*status {
                return Arc::clone(result);
            }
            status = self.changed.wait(status).expect("作业信号锁不应中毒");
        }
    }
    pub fn wait_timeout(&self, timeout: Duration) -> Option<Arc<C>> {
        let status = lock(&self.status);
        let (status, _) = self
            .changed
            .wait_timeout_while(status, timeout, |value| {
                !matches!(value, JobStatus::Finished(_))
            })
            .expect("作业信号锁不应中毒");
        match &*status {
            JobStatus::Finished(result) => Some(Arc::clone(result)),
            _ => None,
        }
    }
}

/// 作业查询句柄。丢弃句柄不取消作业；完成记录只由调用方句柄保留。
#[derive(Clone)]
pub struct OpenJob {
    pub(super) id: OpenJobId,
    pub(super) signal: Arc<JobSignal>,
}

impl OpenJob {
    /// 用于取消和状态关联的作业标识。
    pub fn id(&self) -> OpenJobId {
        self.id
    }
    /// 非阻塞取得当前状态。
    pub fn status(&self) -> JobStatus {
        self.signal.status()
    }

    /// 阻塞到作业完成；不得在 UI 或异步运行时线程直接调用。
    pub fn wait(&self) -> Arc<JobCompletion> {
        self.signal.wait()
    }

    /// 有期限的阻塞等待；超时仅返回 None，不取消后台工作。
    pub fn wait_timeout(&self, timeout: Duration) -> Option<Arc<JobCompletion>> {
        self.signal.wait_timeout(timeout)
    }
}

/// 所有内部锁只保护无外部回调的状态操作；解析器异常在锁外捕获。
/// 中毒代表内部不变量已失效，不将其伪装成可继续使用的正常状态。
pub(super) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().expect("文档服务内部状态锁不应中毒")
}
