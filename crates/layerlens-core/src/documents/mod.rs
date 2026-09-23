//! 多文档权威状态与有界后台打开、预览服务，不依赖 Tauri、MCP 或异步运行时。
//!
//! 单个线程执行路径规范化、同步解析和预览；状态锁只保护注册、取消和发布。
//! 修订和预览不可变，各自引用与资源额度同寿命。取消不会硬中断同步解析或解码。
//! 服务退出会等待正在执行的调用结束，适配层必须在后台执行 shutdown／Drop。

mod budget;
mod cleanup;
mod error;
mod inspection;
mod model;
mod preview;
pub mod selection;
mod state;
mod worker;

pub use cleanup::{CleanupImpact, CleanupPlan, CleanupReceipt};
pub use error::DocumentError;
pub use inspection::{LayerDetails, LayerPage, LayerSummary, TextSlice};
pub use model::{
    DocumentId, DocumentLease, DocumentServiceConfig, DocumentSummary, JobCompletion, JobStatus,
    OpenJob, OpenJobId, OpenOutcome, PreviewJobId, ResourceAccounting, RevisionId, ServiceSnapshot,
};
pub use preview::{
    PreviewAccounting, PreviewCompletion, PreviewConfig, PreviewJob, PreviewKind, PreviewLease,
    PreviewOutcome, PreviewSnapshot,
};

use std::{
    path::Path,
    sync::{Arc, Condvar, Mutex},
    thread::JoinHandle,
    time::Duration,
};

use crate::psd::PsdDocument;
use budget::Budget;
use model::{JobSignal, lock};
use state::{Execution, OpenKind, Shared, State};

/// 一份工作区的文档服务；应用持有它，桌面与 MCP 适配层共享借用。
///
/// 丢弃时取消作业并阻塞等待线程退出；显式 shutdown 更便于应用控制退出阶段。
pub struct DocumentService {
    shared: Arc<Shared>,
    worker: Option<JoinHandle<()>>,
}

impl DocumentService {
    /// 校验额度并启动一个后台线程，不读取 PSD。
    pub fn new(config: DocumentServiceConfig) -> Result<Self, DocumentError> {
        Self::with_loader(config, Box::new(PsdDocument::open))
    }

    fn with_loader(
        config: DocumentServiceConfig,
        loader: worker::Loader,
    ) -> Result<Self, DocumentError> {
        Self::with_workers(config, loader, Box::new(PsdDocument::preview_png_bounded))
    }

    fn with_workers(
        config: DocumentServiceConfig,
        loader: worker::Loader,
        renderer: preview::Renderer,
    ) -> Result<Self, DocumentError> {
        let config = config.validate()?;
        let shared = Arc::new(Shared {
            state: Mutex::new(State::default()),
            ready: Condvar::new(),
            config,
            budget: Budget::new(config),
            preview_budget: preview::budget::Budget::new(config.preview),
            selection_budget: selection::Budget::new(config.selection),
        });
        let worker_state = Arc::clone(&shared);
        let worker = std::thread::Builder::new()
            .name("layerlens-documents".into())
            .spawn(move || worker::run(worker_state, loader, renderer))
            .map_err(DocumentError::WorkerStart)?;
        Ok(Self {
            shared,
            worker: Some(worker),
        })
    }

    /// 提交只读打开。已知路径复用标签／作业；文件系统规范化在后台完成。
    ///
    /// 成功提交后通过句柄查询解析结果。队列满载立即拒绝；实际解析前再预留全局额度。
    /// 后续提交或手动切换优先于本次请求的自动激活。
    pub fn open(&self, path: impl AsRef<Path>) -> Result<OpenJob, DocumentError> {
        let path = path.as_ref();
        let path = std::path::absolute(path).map_err(|source| DocumentError::Path {
            path: path.to_owned(),
            source,
        })?;
        let mut state = lock(&self.shared.state);
        state.ensure_running()?;
        if let Some(entry) = state
            .documents
            .iter()
            .find(|entry| entry.path == path || entry.request_path == path)
        {
            let summary = entry.summary();
            let id = state.allocate_id()?;
            let signal = Arc::new(JobSignal::new());
            signal.set(JobStatus::Finished(Arc::new(JobCompletion {
                outcome: OpenOutcome::Opened {
                    document_id: summary.id,
                    revision_id: summary.revision,
                    reused: true,
                },
                queued_for: Duration::ZERO,
                executed_for: Duration::ZERO,
            })));
            state.set_active(Some(summary.id))?;
            state.activation = None;
            return Ok(OpenJob { id, signal });
        }
        if let Some(job) = state.pending.values().find(|job| {
            job.execution.accepts_cancel()
                && job.kind == OpenKind::Open
                && (job.path == path || job.canonical.as_ref() == Some(&path))
        }) {
            let handle = job.handle.clone();
            state.activation = Some(handle.id);
            return Ok(handle);
        }
        if state.pending_count() >= self.shared.config.max_pending_jobs {
            return Err(DocumentError::QueueFull {
                limit: self.shared.config.max_pending_jobs,
            });
        }
        let id = state.allocate_id()?;
        let handle = state.enqueue(id, DocumentId(id.0), OpenKind::Open, path);
        state.activation = Some(id);
        self.shared.ready.notify_one();
        Ok(handle)
    }

    /// 显式重载，成功后替换当前修订，失败保持旧内容。不会自动切换标签。
    ///
    /// 接受新重载后取消同文档的旧重载；运行中的旧调用仍占用执行名额直到结束。
    pub fn reload(&self, document: DocumentId) -> Result<OpenJob, DocumentError> {
        let mut state = lock(&self.shared.state);
        state.ensure_running()?;
        let path = state
            .documents
            .iter()
            .find(|entry| entry.revision.document_id == document)
            .ok_or(DocumentError::NotFound(document))?
            .path
            .clone();
        let previous: Vec<_> = state
            .pending
            .iter()
            .filter(|(_, job)| {
                job.document == document
                    && job.kind == OpenKind::Reload
                    && job.execution.accepts_cancel()
            })
            .map(|(id, job)| (*id, matches!(job.execution, Execution::Queued)))
            .collect();
        let removable = previous.iter().filter(|(_, queued)| *queued).count();
        if state.pending_count() - removable >= self.shared.config.max_pending_jobs {
            return Err(DocumentError::QueueFull {
                limit: self.shared.config.max_pending_jobs,
            });
        }
        let id = state.allocate_id()?;
        for (old, _) in previous {
            state.cancel(old);
        }
        let handle = state.enqueue(id, document, OpenKind::Reload, path);
        self.shared.ready.notify_one();
        Ok(handle)
    }

    /// 手动激活已发布文档，同时使较早的打开请求失去自动激活资格。
    pub fn activate(&self, document: DocumentId) -> Result<(), DocumentError> {
        let mut state = lock(&self.shared.state);
        state.ensure_running()?;
        if !state
            .documents
            .iter()
            .any(|entry| entry.revision.document_id == document)
        {
            return Err(DocumentError::NotFound(document));
        }
        state.set_active(Some(document))?;
        state.activation = None;
        Ok(())
    }

    /// 关闭查看入口、取消关联加载和预览并使缓存失效；外部修订与 PNG 引用继续有效并计费。
    /// 活动标签优先切换到右邻，否则左邻；解析数据和缓存图像在状态锁外销毁。
    pub fn close(&self, document: DocumentId) -> Result<(), DocumentError> {
        let (removed, cleanup, selections) = {
            let mut state = lock(&self.shared.state);
            state.ensure_running()?;
            let removed = state
                .remove_document(document)?
                .ok_or(DocumentError::NotFound(document))?;
            let related: Vec<_> = state
                .pending
                .iter()
                .filter(|(_, job)| {
                    job.document == document
                        || job.path == removed.request_path
                        || job.path == removed.path
                        || job.canonical.as_ref() == Some(&removed.path)
                })
                .map(|(id, _)| *id)
                .collect();
            for id in related {
                state.cancel(id);
            }
            state.record_close(removed.path.clone());
            let cleanup = state.previews.invalidate(Some(document));
            let selections = state.selections.retire_document(document);
            (removed, cleanup, selections)
        };
        drop(removed);
        drop(selections);
        cleanup.release(&self.shared);
        Ok(())
    }

    /// 取得当前修订的稳定引用；关闭和重载不改变该引用。
    pub fn lease(&self, document: DocumentId) -> Result<DocumentLease, DocumentError> {
        let state = lock(&self.shared.state);
        state.ensure_running()?;
        state
            .documents
            .iter()
            .find(|entry| entry.revision.document_id == document)
            .map(|entry| DocumentLease(Arc::clone(&entry.revision)))
            .ok_or(DocumentError::NotFound(document))
    }

    /// 接受取消返回 true；已经取消、已经发布或不存在的 ID 返回 false。
    pub fn cancel(&self, job: OpenJobId) -> bool {
        lock(&self.shared.state).cancel(job)
    }

    /// 读取轻量状态，不读取源文件或复制文档文字／像素。
    pub fn snapshot(&self) -> ServiceSnapshot {
        let state = lock(&self.shared.state);
        ServiceSnapshot {
            selection: state.selection_summary(),
            documents: state
                .documents
                .iter()
                .map(|entry| entry.summary())
                .collect(),
            active_document: state.active,
            pending_jobs: state
                .pending
                .iter()
                .map(|(id, job)| (*id, job.handle.status()))
                .collect(),
            resources: self.shared.budget.usage(),
            pending_previews: state.previews.statuses(),
            previews: state.previews.snapshot(self.shared.preview_budget.usage()),
            shutting_down: state.stopping,
        }
    }

    /// 停止准入并取消所有作业，在锁外释放预览缓存和排队预览引用。
    /// 不等待同步计算结束或立即销毁文档；之后调用 shutdown 等待线程并释放注册表。
    /// 外部修订与 PNG 引用仍然有效；缓存析构也可能耗时，适配层应在后台调用。
    pub fn request_shutdown(&self) {
        let mut state = lock(&self.shared.state);
        state.stopping = true;
        state.activation = None;
        let ids: Vec<_> = state.pending.keys().copied().collect();
        for id in ids {
            state.cancel(id);
        }
        let cleanup = state.previews.invalidate(None);
        self.shared.ready.notify_one();
        drop(state);
        cleanup.release(&self.shared);
    }

    /// 阻塞等待正在执行的同步计算结束并释放注册表，可重复调用。
    /// 必须放在后台退出流程；不硬中断解析或解码，也不使外部修订与 PNG 引用失效。
    pub fn shutdown(&mut self) -> Result<(), DocumentError> {
        self.request_shutdown();
        let joined = self.worker.take().map(|worker| worker.join());
        let (removed, selections) = {
            let mut state = lock(&self.shared.state);
            state.active = None;
            (
                std::mem::take(&mut state.documents),
                state.selections.drain_references(),
            )
        };
        drop(removed);
        drop(selections);
        if matches!(joined, Some(Err(_))) {
            return Err(DocumentError::WorkerPanicked);
        }
        Ok(())
    }
}

impl Drop for DocumentService {
    fn drop(&mut self) {
        // Drop 无法返回错误；所有工作线程已被 join，显式退出可取得异常原因。
        let _ = self.shutdown();
    }
}

#[cfg(test)]
mod tests;
