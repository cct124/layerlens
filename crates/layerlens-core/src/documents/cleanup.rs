//! 主动资源清理的只读预检和原子确认。计划只持有小型元数据，不保护大文档资源。
//! 确认与选择／任务／加载发布共用短锁，所有强引用与缓存均在锁外释放。

use super::{
    DocumentError, DocumentId, DocumentService, OpenJobId, PreviewJobId, RevisionId,
    model::{Revision, lock},
    selection::{DesignTask, SnapshotId},
    state::{OpenKind, Shared, State},
};
use std::{
    path::PathBuf,
    sync::{
        Arc, Weak,
        atomic::{AtomicBool, Ordering},
    },
};

/// 预检当时的影响集合；任务只含有界摘要，不含原稿内容或强引用。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanupImpact {
    pub document_id: DocumentId,
    pub open_revision: Option<RevisionId>,
    pub selection: Option<SnapshotId>,
    pub live_revisions: Vec<RevisionId>,
    pub tasks: Vec<DesignTask>,
    pub load_jobs: Vec<OpenJobId>,
    /// 尚未规范化且可能是目标路径别名的打开作业；只有最终命中该路径时才取消。
    pub unresolved_load_jobs: Vec<OpenJobId>,
    pub preview_jobs: Vec<PreviewJobId>,
}

/// 不可由调用方构造或改写的确认计划。丢弃等于取消，成功后同一计划可幂等重试。
/// 不序列化此对象；桌面适配层须在有界后台保管它，不能相信客户端回传的影响摘要。
#[derive(Debug)]
pub struct CleanupPlan {
    owner: Weak<Shared>,
    impact: CleanupImpact,
    paths: Vec<PathBuf>,
    applied: AtomicBool,
}
impl CleanupPlan {
    /// 供调用方展示的只读影响摘要。执行前始终重新核对，不以本摘要替代权限检查。
    pub fn impact(&self) -> &CleanupImpact {
        &self.impact
    }
}

/// 仅表示清理决定已应用，不表示同步计算已停止或外部引用占用的资源已归零。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CleanupReceipt {
    pub document_id: DocumentId,
    pub already_applied: bool,
}

// 先存入调用方在状态锁外声明的容器，避免 Weak 升级后最后一个强引用在锁内析构。
fn collect_revisions(state: &State, document: DocumentId, leases: &mut Vec<Arc<Revision>>) {
    for ((id, _), weak) in &state.revisions {
        if *id == document
            && let Some(revision) = weak.upgrade()
        {
            leases.push(revision);
        }
    }
}
fn impact(
    state: &State,
    document: DocumentId,
    leases: &[Arc<Revision>],
) -> Result<(CleanupImpact, Vec<PathBuf>), DocumentError> {
    let entry = state
        .documents
        .iter()
        .find(|entry| entry.revision.document_id == document);
    let revisions: Vec<_> = leases
        .iter()
        .filter(|revision| !revision.invalidated.load(Ordering::Acquire))
        .collect();
    let mut paths: Vec<_> = revisions
        .iter()
        .map(|revision| revision.path.clone())
        .collect();
    if let Some(entry) = entry {
        paths.push(entry.path.clone());
        paths.push(entry.request_path.clone());
    }
    let tasks = state.selections.cleanup_tasks(document);
    // 同路径别名仅随打开的目标标签取消；旧任务所属文档关闭后重开的新 ID 不受影响。
    let load_jobs: Vec<_> = state
        .pending
        .iter()
        .filter(|(_, job)| {
            job.document == document
                || (entry.is_some()
                    && (paths.contains(&job.path)
                        || job
                            .canonical
                            .as_ref()
                            .is_some_and(|path| paths.contains(path))))
        })
        .map(|(id, _)| *id)
        .collect();
    let preview_jobs = state.previews.cleanup_jobs(document);
    // 预检后新增的未知别名不能被确认时的关闭截止点悄悄纳入取消范围。
    // 保守记录全部未解析候选；无关路径只影响确认是否过期，不直接取消它们。
    let unresolved_load_jobs = state
        .pending
        .iter()
        .filter(|(id, job)| {
            entry.is_some()
                && job.kind == OpenKind::Open
                && job.canonical.is_none()
                && !load_jobs.contains(id)
        })
        .map(|(id, _)| *id)
        .collect();
    if entry.is_none()
        && revisions.is_empty()
        && tasks.is_empty()
        && load_jobs.is_empty()
        && preview_jobs.is_empty()
    {
        return Err(DocumentError::NotFound(document));
    }
    paths.sort();
    paths.dedup();
    Ok((
        CleanupImpact {
            document_id: document,
            open_revision: entry.map(|entry| entry.revision.id),
            selection: entry
                .and_then(|entry| entry.selection.as_ref().map(|selection| selection.id())),
            live_revisions: revisions.iter().map(|revision| revision.id).collect(),
            tasks,
            load_jobs,
            unresolved_load_jobs,
            preview_jobs,
        },
        paths,
    ))
}

impl DocumentService {
    /// 只读列出同文档 ID 的标签、所有存活修订、活跃任务及已知在途工作。
    /// 包括已关闭标签而仍被任务保护的文档；不会读取源路径、创建任务或固定资源。
    /// 临时升级的弱引用可能成为最后一个引用，预检同样应在有界后台执行。
    ///
    /// # Errors
    /// 不存在可清理对象或服务退出时拒绝。计划期间资源释放或影响变化需重新预检。
    pub fn prepare_cleanup(&self, document: DocumentId) -> Result<CleanupPlan, DocumentError> {
        let mut leases = Vec::new();
        let state = lock(&self.shared.state);
        state.ensure_running()?;
        collect_revisions(&state, document, &mut leases);
        let (impact, paths) = impact(&state, document, &leases)?;
        Ok(CleanupPlan {
            owner: Arc::downgrade(&self.shared),
            impact,
            paths,
            applied: AtomicBool::new(false),
        })
    }

    /// 确认预检计划：短锁内重检后关闭标签、失效修订／快照／活跃任务并取消加载和预览。
    /// 普通释放的任务保持 Released；已发布结果与磁盘 PSD 均不删除。
    /// 大对象在锁外析构，本方法应在有界后台调用，不在 UI／异步执行线程直接运行。
    ///
    /// # Errors
    /// 跨实例、影响变化或退出时拒绝，失败不执行部分清理。已成功计划重复执行只返回回执。
    /// 取消不硬中断同步解析，存活外部引用继续计费；不保证调用返回时内存已回收。
    pub fn commit_cleanup(&self, plan: &CleanupPlan) -> Result<CleanupReceipt, DocumentError> {
        if !Weak::ptr_eq(&plan.owner, &Arc::downgrade(&self.shared)) {
            return Err(DocumentError::ForeignCleanupPlan);
        }
        let mut leases = Vec::new();
        let document = plan.impact.document_id;
        let (removed, snapshots, cleanup) = {
            let mut state = lock(&self.shared.state);
            state.ensure_running()?;
            if plan.applied.load(Ordering::Acquire) {
                return Ok(CleanupReceipt {
                    document_id: document,
                    already_applied: true,
                });
            }
            collect_revisions(&state, document, &mut leases);
            let (current, paths) =
                impact(&state, document, &leases).map_err(|_| DocumentError::StaleCleanupPlan)?;
            if current != plan.impact || paths != plan.paths {
                return Err(DocumentError::StaleCleanupPlan);
            }
            // 选择版本耗尽也必须发生在任何失效或取消之前。后续动作均不会失败。
            let removed = state.remove_document(document)?;
            for revision in &leases {
                revision.invalidated.store(true, Ordering::Release);
            }
            for job in &current.load_jobs {
                state.cancel(*job);
                if state.activation == Some(*job) {
                    state.activation = None;
                }
            }
            if removed.is_some() {
                for path in paths {
                    state.record_close(path);
                }
            }
            let cleanup = state.previews.invalidate(Some(document));
            let snapshots = state.selections.invalidate_document(document);
            state.revisions.retain(|_, weak| weak.strong_count() > 0);
            plan.applied.store(true, Ordering::Release);
            (removed, snapshots, cleanup)
        };
        drop(removed);
        drop(snapshots);
        cleanup.release(&self.shared);
        drop(leases);
        Ok(CleanupReceipt {
            document_id: document,
            already_applied: false,
        })
    }
}
