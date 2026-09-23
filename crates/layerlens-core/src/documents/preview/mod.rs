//! 原尺寸保存时预览调度、LRU 缓存与生命周期失效；与打开作业共用工作线程。

pub(super) mod budget;
mod model;
mod worker;

pub use model::{
    PreviewAccounting, PreviewCompletion, PreviewConfig, PreviewJob, PreviewKind, PreviewLease,
    PreviewOutcome, PreviewSnapshot,
};
pub(super) use worker::{Renderer, Work, render};

use super::{
    DocumentError, DocumentId, DocumentService, JobStatus, PreviewJobId,
    model::{JobSignal, Revision, lock},
    state::{Execution, Shared},
};
use model::Key;
use std::{
    collections::{BTreeMap, VecDeque},
    sync::Arc,
    time::{Duration, Instant},
};

pub(super) struct Pending {
    handle: PreviewJob,
    key: Key,
    revision: Option<Arc<Revision>>,
    execution: Execution,
    queued_at: Instant,
}
impl Pending {
    fn finish(self, outcome: PreviewOutcome, timings: Option<crate::psd::PngTimings>) {
        let now = Instant::now();
        let started = self.execution.started_at();
        self.handle
            .signal
            .set(JobStatus::Finished(Arc::new(PreviewCompletion {
                outcome,
                cache_hit: false,
                queued_for: started.unwrap_or(now).duration_since(self.queued_at),
                executed_for: started.map_or(Duration::ZERO, |at| now.duration_since(at)),
                png_timings: timings,
            })));
    }
}

#[derive(Default)]
pub(super) struct State {
    pub pending: BTreeMap<PreviewJobId, Pending>,
    pub queue: VecDeque<PreviewJobId>,
    cache: VecDeque<PreviewLease>,
    cache_bytes: u64,
    cache_hits: u64,
    coalesced_requests: u64,
    evictions: u64,
}
impl State {
    pub(super) fn cleanup_jobs(&self, document: DocumentId) -> Vec<PreviewJobId> {
        self.pending
            .iter()
            .filter(|(_, job)| job.key.document == document)
            .map(|(id, _)| *id)
            .collect()
    }
    pub fn statuses(&self) -> Vec<(PreviewJobId, JobStatus<PreviewCompletion>)> {
        self.pending
            .iter()
            .map(|(id, job)| (*id, job.handle.status()))
            .collect()
    }
    pub fn snapshot(&self, resources: PreviewAccounting) -> PreviewSnapshot {
        PreviewSnapshot {
            resources,
            cache_entries: self.cache.len(),
            cache_bytes: self.cache_bytes,
            cache_hits: self.cache_hits,
            coalesced_requests: self.coalesced_requests,
            evictions: self.evictions,
        }
    }
    fn cached(&mut self, key: Key) -> Option<PreviewLease> {
        let index = self.cache.iter().position(|entry| entry.0.key == key)?;
        let entry = self.cache.remove(index).expect("命中项必须存在");
        let result = entry.clone();
        self.cache.push_back(entry);
        self.cache_hits = self.cache_hits.saturating_add(1);
        Some(result)
    }
    fn evict(&mut self) -> Option<PreviewLease> {
        let entry = self.cache.pop_front()?;
        self.cache_bytes -= entry.charged_bytes();
        self.evictions = self.evictions.saturating_add(1);
        Some(entry)
    }
    fn insert(&mut self, image: &PreviewLease, config: PreviewConfig) -> Vec<PreviewLease> {
        let mut removed = Vec::new();
        if config.max_cache_entries == 0 || image.charged_bytes() > config.max_cache_bytes {
            return removed;
        }
        while self.cache.len() >= config.max_cache_entries
            || image.charged_bytes() > config.max_cache_bytes.saturating_sub(self.cache_bytes)
        {
            removed.push(self.evict().expect("容量超限时缓存非空"));
        }
        self.cache_bytes += image.charged_bytes();
        self.cache.push_back(image.clone());
        removed
    }
    /// 运行中只标记取消；排队项转 Finishing，先移出数据再在锁外销毁。
    fn cancel(&mut self, id: PreviewJobId, cleanup: &mut Cleanup) -> bool {
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
                job.execution = Execution::Finishing(Instant::now());
                job.handle.signal.set(JobStatus::Finishing);
                self.queue.retain(|queued| *queued != id);
                cleanup
                    .queued
                    .push((id, job.revision.take().expect("排队预览持有修订")));
            }
        }
        true
    }
    pub fn invalidate(&mut self, document: Option<DocumentId>) -> Cleanup {
        let mut cleanup = Cleanup::default();
        let ids: Vec<_> = self
            .pending
            .iter()
            .filter(|(_, job)| document.is_none_or(|id| job.key.document == id))
            .map(|(id, _)| *id)
            .collect();
        for id in ids {
            self.cancel(id, &mut cleanup);
        }
        let mut keep = VecDeque::new();
        while let Some(entry) = self.cache.pop_front() {
            if document.is_none_or(|id| entry.document_id() == id) {
                self.cache_bytes -= entry.charged_bytes();
                cleanup.images.push(entry);
            } else {
                keep.push_back(entry);
            }
        }
        self.cache = keep;
        cleanup
    }
}

#[derive(Default)]
pub(super) struct Cleanup {
    queued: Vec<(PreviewJobId, Arc<Revision>)>,
    images: Vec<PreviewLease>,
}
impl Cleanup {
    pub fn release(self, shared: &Shared) {
        drop(self.images);
        for (id, revision) in self.queued {
            drop(revision);
            let job = lock(&shared.state)
                .previews
                .pending
                .remove(&id)
                .expect("释放前保留预览名额");
            // 排队取消从未执行，计时保留完整排队时长。
            let mut job = job;
            job.execution = Execution::Queued;
            job.finish(PreviewOutcome::Cancelled, None);
        }
    }
}

impl DocumentService {
    /// 请求当前修订的原尺寸保存时合成图。缓存命中不占作业名额；同修订在途请求合并。
    ///
    /// 请求固定提交时修订，重载成功或关闭时取消过期工作；失败重载不使旧预览失效。
    /// 预算在实际执行前预留；命中结果附带能力限制，调用方不能把 partial 当作完整渲染。
    pub fn preview(&self, document: DocumentId) -> Result<PreviewJob, DocumentError> {
        let mut state = lock(&self.shared.state);
        state.ensure_running()?;
        let revision = &state
            .documents
            .iter()
            .find(|entry| entry.revision.document_id == document)
            .ok_or(DocumentError::NotFound(document))?
            .revision;
        let key = Key {
            document,
            revision: revision.id,
            kind: PreviewKind::SavedCompositePng,
        };
        if let Some(job) = state
            .previews
            .pending
            .values()
            .find(|job| job.key == key && job.execution.accepts_cancel())
        {
            let handle = job.handle.clone();
            state.previews.coalesced_requests = state.previews.coalesced_requests.saturating_add(1);
            return Ok(handle);
        }
        // 分配 ID 不代表占用执行名额；缓存命中即使队列满也可以返回。
        let id = PreviewJobId(state.allocate_id()?.get());
        let handle = PreviewJob {
            id,
            signal: Arc::new(JobSignal::new()),
        };
        if let Some(image) = state.previews.cached(key) {
            handle
                .signal
                .set(JobStatus::Finished(Arc::new(PreviewCompletion {
                    outcome: PreviewOutcome::Ready(image),
                    cache_hit: true,
                    queued_for: Duration::ZERO,
                    executed_for: Duration::ZERO,
                    png_timings: None,
                })));
            return Ok(handle);
        }
        if state.pending_count() >= self.shared.config.max_pending_jobs {
            return Err(DocumentError::QueueFull {
                limit: self.shared.config.max_pending_jobs,
            });
        }
        let revision = Arc::clone(
            &state
                .documents
                .iter()
                .find(|entry| entry.revision.document_id == document)
                .expect("同一状态锁内文档仍存在")
                .revision,
        );
        state.previews.pending.insert(
            id,
            Pending {
                handle: handle.clone(),
                key,
                revision: Some(revision),
                execution: Execution::Queued,
                queued_at: Instant::now(),
            },
        );
        state.previews.queue.push_back(id);
        self.shared.ready.notify_one();
        Ok(handle)
    }
    /// 取消合并的整个预览作业；同步解码不能硬中断，返回 true 不表示资源已经释放。
    pub fn cancel_preview(&self, id: PreviewJobId) -> bool {
        let mut cleanup = Cleanup::default();
        let accepted = lock(&self.shared.state).previews.cancel(id, &mut cleanup);
        cleanup.release(&self.shared);
        accepted
    }
}

#[cfg(test)]
mod tests;
