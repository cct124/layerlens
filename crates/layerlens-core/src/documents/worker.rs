//! 单线程按 FIFO 执行路径解析、PSD 读取、解析和预览；计算和状态发布分开。

use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Instant,
};

use crate::psd::{ParseLimits, PsdDocument, PsdError};

use super::{
    DocumentError, DocumentId, JobStatus, OpenJobId, OpenOutcome, RevisionId,
    model::{Revision, lock},
    preview,
    state::{Entry, Execution, OpenKind, Shared},
};

pub(super) type Loader = Box<dyn Fn(&Path, ParseLimits) -> Result<PsdDocument, PsdError> + Send>;

struct Work {
    id: OpenJobId,
    document: DocumentId,
    kind: OpenKind,
    path: PathBuf,
}

enum Computed {
    Reused {
        document: DocumentId,
        revision: RevisionId,
    },
    Loaded {
        path: PathBuf,
        revision: Arc<Revision>,
    },
    Cancelled,
}

enum Next {
    Open(Work),
    Preview(preview::Work),
}

pub(super) fn run(shared: Arc<Shared>, loader: Loader, renderer: preview::Renderer) {
    loop {
        let work = {
            let mut state = lock(&shared.state);
            while state.queue.is_empty() && state.previews.queue.is_empty() && !state.stopping {
                state = shared.ready.wait(state).expect("文档服务状态锁不应中毒");
            }
            if state.stopping {
                return;
            }
            // 两类作业共用递增 ID，按提交顺序执行；缓存命中不进入队列。
            if state
                .previews
                .queue
                .front()
                .is_some_and(|id| state.queue.front().is_none_or(|open| id.get() < open.get()))
            {
                Next::Preview(preview::Work::start(&mut state.previews))
            } else {
                // 队列与 pending 仅在同一个状态锁下变更。
                let id = state.queue.pop_front().expect("非空队列必须有首项");
                let job = state.pending.get_mut(&id).expect("队列项必须有作业记录");
                job.execution = Execution::Running(Instant::now());
                job.handle.signal.set(JobStatus::Running);
                Next::Open(Work {
                    id,
                    document: job.document,
                    kind: job.kind,
                    path: job.path.clone(),
                })
            }
        };
        let work = match work {
            Next::Preview(work) => {
                preview::render(&shared, work, &renderer);
                continue;
            }
            Next::Open(work) => work,
        };
        // 第三方解析 panic 不能使等待者永久悬挂；展开栈会释放预留和未发布数据。
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            compute(&shared, &work, &loader)
        }))
        .unwrap_or(Err(DocumentError::WorkerPanicked));
        publish(&shared, work, result);
    }
}

fn compute(shared: &Shared, work: &Work, loader: &Loader) -> Result<Computed, DocumentError> {
    let path = std::fs::canonicalize(&work.path).map_err(|source| DocumentError::Path {
        path: work.path.clone(),
        source,
    })?;
    {
        let mut state = lock(&shared.state);
        if state
            .closed_paths
            .iter()
            .any(|(closed, cutoff)| closed == &path && work.id <= *cutoff)
        {
            return Ok(Computed::Cancelled);
        }
        let job = state
            .pending
            .get_mut(&work.id)
            .expect("运行中的作业直到发布前保留记录");
        if matches!(job.execution, Execution::Cancelling(_)) {
            return Ok(Computed::Cancelled);
        }
        job.canonical = Some(path.clone());
        if work.kind == OpenKind::Open
            && let Some(entry) = state.documents.iter().find(|entry| entry.path == path)
        {
            return Ok(Computed::Reused {
                document: entry.revision.document_id,
                revision: entry.revision.id,
            });
        }
    }
    let mut permit = shared.budget.reserve()?;
    if lock(&shared.state)
        .pending
        .get(&work.id)
        .is_none_or(|job| matches!(job.execution, Execution::Cancelling(_)))
    {
        return Ok(Computed::Cancelled);
    }
    let parsed =
        loader(&path, shared.config.parse_limits).map_err(|source| DocumentError::Parse {
            path: path.clone(),
            source,
        })?;
    permit.settle(
        parsed.info().resources.source_bytes,
        parsed.info().resources.total_declared_pixels,
    )?;
    Ok(Computed::Loaded {
        path,
        revision: Arc::new(Revision {
            document_id: work.document,
            id: RevisionId(work.id.0),
            parsed,
            _permit: permit,
        }),
    })
}

fn publish(shared: &Shared, work: Work, result: Result<Computed, DocumentError>) {
    // 任何未发布的新修订或被替换的旧修订都必须在离开状态锁后销毁。
    let mut discarded = None;
    let mut cleanup = preview::Cleanup::default();
    let outcome = {
        let mut state = lock(&shared.state);
        let job = state
            .pending
            .get_mut(&work.id)
            .expect("发布前必须保留运行中的作业");
        let cancelled = matches!(job.execution, Execution::Cancelling(_));
        let started = job
            .execution
            .started_at()
            .expect("发布作业必须已经开始执行");
        job.execution = Execution::Finishing(started);
        job.handle.signal.set(JobStatus::Finishing);
        let cancelled = cancelled
            || state.stopping
            || (work.kind == OpenKind::Reload
                && !state
                    .documents
                    .iter()
                    .any(|entry| entry.revision.document_id == work.document));
        let outcome = if cancelled {
            if let Ok(Computed::Loaded { revision, .. }) = result {
                discarded = Some(revision);
            }
            OpenOutcome::Cancelled
        } else {
            match result {
                Err(error) => OpenOutcome::Failed(Arc::new(error)),
                Ok(Computed::Cancelled) => OpenOutcome::Cancelled,
                Ok(Computed::Reused { document, revision }) => {
                    if state
                        .documents
                        .iter()
                        .any(|entry| entry.revision.document_id == document)
                    {
                        if state.activation == Some(work.id) {
                            state.active = Some(document);
                        }
                        OpenOutcome::Opened {
                            document_id: document,
                            revision_id: revision,
                            reused: true,
                        }
                    } else {
                        OpenOutcome::Cancelled
                    }
                }
                Ok(Computed::Loaded { path, revision }) => {
                    let revision_id = revision.id;
                    if work.kind == OpenKind::Reload {
                        let entry = state
                            .documents
                            .iter_mut()
                            .find(|entry| entry.revision.document_id == work.document)
                            .expect("未取消重载的目标必须存在");
                        discarded = Some(std::mem::replace(&mut entry.revision, revision));
                        cleanup = state.previews.invalidate(Some(work.document));
                    } else {
                        state.documents.push(Entry {
                            path,
                            request_path: work.path,
                            revision,
                        });
                        if state.activation == Some(work.id) {
                            state.active = Some(work.document);
                        }
                    }
                    OpenOutcome::Opened {
                        document_id: work.document,
                        revision_id,
                        reused: false,
                    }
                }
            }
        };
        if state.activation == Some(work.id) {
            state.activation = None;
        }
        outcome
    };
    drop(discarded);
    cleanup.release(shared);
    let mut state = lock(&shared.state);
    // 释放结束前保留作业占位，避免销毁大修订时提前归还执行名额。
    let job = state
        .pending
        .remove(&work.id)
        .expect("释放期间保留运行中的作业");
    state.prune_closed_paths();
    // 完成信号发生在取消结果／旧修订的资源释放之后，wait 返回时账本已稳定。
    job.finish(outcome);
}
