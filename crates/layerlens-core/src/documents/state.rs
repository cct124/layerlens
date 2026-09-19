//! 短锁内的注册表、激活意图和作业转换，不做文件访问或销毁解析数据。

use std::{
    collections::{BTreeMap, VecDeque},
    path::PathBuf,
    sync::{Arc, Condvar, Mutex},
    time::{Duration, Instant},
};

use super::{
    DocumentError, DocumentId, DocumentServiceConfig, DocumentSummary, JobCompletion, JobStatus,
    OpenJob, OpenJobId, OpenOutcome,
    budget::Budget,
    model::{JobSignal, Revision},
};

pub(super) struct Entry {
    pub path: PathBuf,
    pub request_path: PathBuf,
    pub revision: Arc<Revision>,
    pub selected_layer: Option<crate::psd::LayerId>,
}

impl Entry {
    pub fn summary(&self) -> DocumentSummary {
        DocumentSummary {
            id: self.revision.document_id,
            revision: self.revision.id,
            path: self.path.clone(),
            width: self.revision.parsed.info().width,
            height: self.revision.parsed.info().height,
            layer_count: self.revision.parsed.info().resources.layer_records,
            color_mode: self.revision.parsed.info().color_mode.clone(),
            bit_depth: self.revision.parsed.info().bit_depth,
            preview_note: self.revision.parsed.info().preview.reason.clone(),
            selected_layer: self.selected_layer,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum OpenKind {
    Open,
    Reload,
}

#[derive(Clone, Copy)]
pub(super) enum Execution {
    Queued,
    Running(Instant),
    Cancelling(Instant),
    Finishing(Instant),
}

impl Execution {
    pub fn started_at(self) -> Option<Instant> {
        match self {
            Self::Queued => None,
            Self::Running(at) | Self::Cancelling(at) | Self::Finishing(at) => Some(at),
        }
    }
    pub fn accepts_cancel(self) -> bool {
        matches!(self, Self::Queued | Self::Running(_))
    }
}

pub(super) struct Pending {
    pub handle: OpenJob,
    pub document: DocumentId,
    pub kind: OpenKind,
    pub path: PathBuf,
    pub canonical: Option<PathBuf>,
    pub execution: Execution,
    pub queued_at: Instant,
}

impl Pending {
    pub fn finish(self, outcome: OpenOutcome) {
        let now = Instant::now();
        let started_at = self.execution.started_at();
        let started = started_at.unwrap_or(now);
        self.handle
            .signal
            .set(JobStatus::Finished(Arc::new(JobCompletion {
                outcome,
                queued_for: started.duration_since(self.queued_at),
                executed_for: started_at.map_or(Duration::ZERO, |time| now.duration_since(time)),
            })));
    }
}

#[derive(Default)]
pub(super) struct State {
    pub documents: Vec<Entry>,
    pub active: Option<DocumentId>,
    /// 只有最新打开意图的完成可以激活；手动切换／关闭活动入口会清除此意图。
    pub activation: Option<OpenJobId>,
    pub pending: BTreeMap<OpenJobId, Pending>,
    pub queue: VecDeque<OpenJobId>,
    pub previews: super::preview::State,
    pub stopping: bool,
    next_job: u64,
    /// 关闭时尚未规范化的路径别名不得重新打开标签。仅保留仍有早期作业的记录。
    pub closed_paths: Vec<(PathBuf, OpenJobId)>,
}

impl State {
    pub fn pending_count(&self) -> usize {
        self.pending.len() + self.previews.pending.len()
    }
    pub fn ensure_running(&self) -> Result<(), DocumentError> {
        if self.stopping {
            Err(DocumentError::ShuttingDown)
        } else {
            Ok(())
        }
    }

    pub fn allocate_id(&mut self) -> Result<OpenJobId, DocumentError> {
        self.next_job = self
            .next_job
            .checked_add(1)
            .ok_or(DocumentError::IdExhausted)?;
        Ok(OpenJobId(self.next_job))
    }

    pub fn enqueue(
        &mut self,
        id: OpenJobId,
        document: DocumentId,
        kind: OpenKind,
        path: PathBuf,
    ) -> OpenJob {
        let handle = OpenJob {
            id,
            signal: Arc::new(JobSignal::new()),
        };
        self.pending.insert(
            id,
            Pending {
                handle: handle.clone(),
                document,
                kind,
                path,
                canonical: None,
                execution: Execution::Queued,
                queued_at: Instant::now(),
            },
        );
        self.queue.push_back(id);
        handle
    }

    /// 返回 true 表示这次取消先于结果发布；运行中的作业仍在 pending 中占位。
    pub fn cancel(&mut self, id: OpenJobId) -> bool {
        let Some(job) = self.pending.get_mut(&id) else {
            return false;
        };
        match job.execution {
            Execution::Cancelling(_) | Execution::Finishing(_) => return false,
            Execution::Running(at) => {
                job.execution = Execution::Cancelling(at);
                job.handle.signal.set(JobStatus::Cancelling);
            }
            Execution::Queued => {
                self.queue.retain(|queued| *queued != id);
                // 上面已确认该项存在，且当前持有唯一状态写锁。
                self.pending
                    .remove(&id)
                    .expect("取消的排队作业必须存在")
                    .finish(OpenOutcome::Cancelled);
                self.prune_closed_paths();
            }
        }
        true
    }

    pub fn record_close(&mut self, path: PathBuf) {
        self.closed_paths.push((path, OpenJobId(self.next_job)));
        self.prune_closed_paths();
    }

    pub fn prune_closed_paths(&mut self) {
        self.closed_paths
            .retain(|(_, cutoff)| self.pending.keys().any(|id| id <= cutoff));
    }
}

pub(super) struct Shared {
    pub state: Mutex<State>,
    pub ready: Condvar,
    pub config: DocumentServiceConfig,
    pub budget: Arc<Budget>,
    pub preview_budget: Arc<super::preview::budget::Budget>,
}
