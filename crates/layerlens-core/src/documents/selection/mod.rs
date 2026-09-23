//! 用户选区、固定范围和设计任务，复用文档服务短锁及不可变修订。
//! 范围计算／内容分页在锁外；注册表只移动引用，最后引用及源数据在锁外释放。
//! 查看器 selected_layer 与这里的选区无授权关系；桌面及未来协议共用同一服务。

mod budget;
mod content;
mod error;
mod handle;
mod model;
mod normalize;
mod tasks;

pub(super) use budget::Budget;
pub use content::{ContentLayer, ContentPage};
pub use error::SelectionError;
pub use handle::SelectionHandle;
pub use model::{
    ContentCursor, DesignTask, DocumentReference, IntersectionKind, LayerIntersection,
    LayerReference, LayerRole, PreparedSelection, SelectionAccounting, SelectionBounds,
    SelectionConfig, SelectionInput, SelectionItem, SelectionRevision, SelectionScope,
    SelectionSummary, SelectionWarning, SessionId, SnapshotBrief, SnapshotDocumentInfo, SnapshotId,
    SnapshotLease, TaskId, TaskPage, TaskStatus, UserSelection,
};

use std::{
    collections::{BTreeMap, VecDeque},
    sync::{Arc, Weak},
};

use super::{DocumentError, DocumentId, DocumentLease, DocumentService, model::lock, state};
use model::Snapshot;

pub(super) struct State {
    session_id: SessionId,
    revision: SelectionRevision,
    next_snapshot: u64,
    next_task: u64,
    snapshots: BTreeMap<SnapshotId, Weak<Snapshot>>,
    cache: VecDeque<SnapshotLease>,
    tasks: BTreeMap<TaskId, tasks::TaskEntry>,
    requests: BTreeMap<String, TaskId>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            session_id: SessionId::new(),
            revision: SelectionRevision(0),
            next_snapshot: 0,
            next_task: 0,
            snapshots: BTreeMap::new(),
            cache: VecDeque::new(),
            tasks: BTreeMap::new(),
            requests: BTreeMap::new(),
        }
    }
}

impl State {
    pub(super) fn cleanup_tasks(&self, document: DocumentId) -> Vec<DesignTask> {
        self.tasks
            .values()
            .filter(|task| {
                task.record.scope.document_id == document
                    && task.record.status == TaskStatus::Active
            })
            .map(|task| task.record.clone())
            .collect()
    }

    /// 所有旧修订快照都撤销 ID 查找；在途强引用只标记失效，最后析构必须留在锁外。
    pub(super) fn invalidate_document(&mut self, document: DocumentId) -> Vec<SnapshotLease> {
        let mut removed = self.retire_document(document);
        for task in self.tasks.values_mut() {
            if task.record.scope.document_id == document && task.record.status == TaskStatus::Active
            {
                task.record.status = TaskStatus::Invalidated;
                if let Some(snapshot) = task.snapshot.take() {
                    removed.push(snapshot);
                }
            }
        }
        self.snapshots.retain(|_, weak| {
            let Some(snapshot) = weak.upgrade() else {
                return false;
            };
            let snapshot = SnapshotLease(snapshot);
            let keep = snapshot.document_id() != document;
            // 包括非目标的临时升级引用，避免并发释放时在锁内销毁最后一份数据。
            removed.push(snapshot);
            keep
        });
        removed
    }

    /// 退出后不再接受 ID 查找，释放所有内部强引用；保留会话标识供诊断。
    pub(super) fn drain_references(&mut self) -> Vec<SnapshotLease> {
        let mut removed: Vec<_> = self.cache.drain(..).collect();
        for (_, mut task) in std::mem::take(&mut self.tasks) {
            if let Some(snapshot) = task.snapshot.take() {
                removed.push(snapshot);
            }
        }
        self.requests.clear();
        self.snapshots.clear();
        removed
    }

    pub(super) fn advance(&mut self) -> Result<(), DocumentError> {
        self.revision = SelectionRevision(
            self.revision
                .0
                .checked_add(1)
                .ok_or(DocumentError::IdExhausted)?,
        );
        Ok(())
    }

    fn check_session(&self, session: &SessionId) -> Result<(), SelectionError> {
        if session != &self.session_id {
            return Err(SelectionError::ForeignSession);
        }
        Ok(())
    }

    fn snapshot(&self, id: SnapshotId) -> Result<SnapshotLease, SelectionError> {
        self.snapshots
            .get(&id)
            .and_then(Weak::upgrade)
            .map(SnapshotLease)
            .ok_or(SelectionError::SnapshotExpired)
    }

    /// 清理不再适用的未绑定缓存；返回给调用方在状态锁外释放。
    pub(super) fn retire_document(&mut self, document: DocumentId) -> Vec<SnapshotLease> {
        let (removed, kept) = self
            .cache
            .drain(..)
            .partition(|snapshot| snapshot.document_id() == document);
        self.cache = kept;
        self.snapshots.retain(|_, weak| weak.strong_count() > 0);
        removed.into()
    }

    fn cache_previous(
        &mut self,
        previous: Option<SnapshotLease>,
        limit: usize,
    ) -> Vec<SnapshotLease> {
        if let Some(previous) = previous {
            self.cache.push_back(previous);
        }
        let count = self.cache.len().saturating_sub(limit);
        self.cache.drain(..count).collect()
    }
}

impl state::State {
    /// 所有激活路径均从这里推进选择版本；重复激活不产生新版本。
    pub(super) fn set_active(&mut self, document: Option<DocumentId>) -> Result<(), DocumentError> {
        if self.active != document {
            self.selections.advance()?;
            self.active = document;
        }
        Ok(())
    }

    fn capture_selection(&self) -> UserSelection {
        let entry = self
            .documents
            .iter()
            .find(|entry| Some(entry.revision.document_id) == self.active);
        UserSelection {
            session_id: self.selections.session_id.clone(),
            revision: self.selections.revision,
            document_id: entry.map(|entry| entry.revision.document_id),
            document_revision: entry.map(|entry| entry.revision.id),
            snapshot: entry.and_then(|entry| entry.selection.clone()),
        }
    }

    // 与文档列表在同一次锁内投影，不将可能延长大修订生命的引用交给通知线程。
    pub(super) fn selection_summary(&self) -> SelectionSummary {
        let entry = self
            .documents
            .iter()
            .find(|entry| Some(entry.revision.document_id) == self.active);
        let snapshot = entry.and_then(|entry| entry.selection.as_ref());
        SelectionSummary {
            session_id: self.selections.session_id.clone(),
            revision: self.selections.revision,
            document_id: entry.map(|entry| entry.revision.document_id),
            document_revision: entry.map(|entry| entry.revision.id),
            snapshot: snapshot.map(SnapshotLease::brief),
            items: snapshot.map_or_else(Vec::new, |snapshot| snapshot.scope().items.clone()),
        }
    }

    fn validate_selection(
        &self,
        lease: &DocumentLease,
        expected: SelectionRevision,
    ) -> Result<usize, SelectionError> {
        self.ensure_running()?;
        lease.ensure_valid()?;
        let index = self
            .documents
            .iter()
            .position(|entry| entry.revision.document_id == lease.document_id())
            .ok_or(DocumentError::NotFound(lease.document_id()))?;
        if !Arc::ptr_eq(&self.documents[index].revision, &lease.0) {
            return Err(DocumentError::StaleRevision.into());
        }
        if self.active != Some(lease.document_id()) {
            return Err(SelectionError::InactiveDocument);
        }
        if self.selections.revision != expected {
            return Err(SelectionError::StaleSelection);
        }
        Ok(index)
    }
}

impl SelectionHandle {
    /// 原子采样活动文档、选择版本与快照强引用。区域内容必须从返回引用继续读取。
    pub fn user_selection(&self) -> Result<UserSelection, SelectionError> {
        let state = lock(&self.shared.state);
        state.ensure_running()?;
        Ok(state.capture_selection())
    }

    /// 在固定修订上准备规范化范围，不改变当前选择，不创建可绑定的快照。
    ///
    /// # Errors
    /// 旧修订、旧选择版本、非活动文档、跨文档图层、无效矩形或超限请求均被拒绝。
    /// 计算可能扫描全部元数据，适配层应在有界后台执行，不在指针移动时逐帧调用。
    pub fn prepare_selection(
        &self,
        lease: &DocumentLease,
        expected: SelectionRevision,
        input: SelectionInput,
    ) -> Result<PreparedSelection, SelectionError> {
        let document_name = {
            let state = lock(&self.shared.state);
            let index = state.validate_selection(lease, expected)?;
            // 终态任务保留有界显示标签；名称不是定位或授权依据。
            let name = state.documents[index]
                .path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy();
            let mut end = name.len().min(256);
            while !name.is_char_boundary(end) {
                end -= 1;
            }
            name[..end].to_owned()
        };
        let scope = normalize::normalize(
            lease.info(),
            lease.document_id(),
            lease.revision_id(),
            input,
            self.shared.config.selection,
        )?;
        lease.ensure_valid()?;
        Ok(PreparedSelection {
            document: lease.clone(),
            expected_revision: expected,
            scope,
            document_name,
        })
    }

    /// 原子发布已准备范围；提交前再次核对修订与选择版本。相同规范化选择不新建快照。
    ///
    /// # Errors
    /// 准备后发生的切换、重载、关闭、选区变化或额度不足使本次失败，旧选择不变。
    /// 资源不足时只清理可回收缓存，不释放当前选区、活跃任务或在途读取的引用。
    pub fn commit_selection(
        &self,
        prepared: PreparedSelection,
    ) -> Result<UserSelection, SelectionError> {
        {
            let state = lock(&self.shared.state);
            let index = state.validate_selection(&prepared.document, prepared.expected_revision)?;
            if state.documents[index]
                .selection
                .as_ref()
                .is_some_and(|current| current.scope().items == prepared.scope.items)
            {
                return Ok(state.capture_selection());
            }
        }
        let bytes = prepared.scope.charged_bytes() + prepared.document_name.capacity() as u64;
        let permit = match self.shared.selection_budget.reserve(bytes) {
            Ok(permit) => permit,
            Err(_) => {
                // 锁外释放缓存后只重试一次；活跃引用不会被回收，无法容纳则明确失败。
                let removed = std::mem::take(&mut lock(&self.shared.state).selections.cache);
                drop(removed);
                self.shared.selection_budget.reserve(bytes)?
            }
        };
        let (selection, removed) = {
            let mut state = lock(&self.shared.state);
            let index = state.validate_selection(&prepared.document, prepared.expected_revision)?;
            let id = SnapshotId(
                state
                    .selections
                    .next_snapshot
                    .checked_add(1)
                    .ok_or(DocumentError::IdExhausted)?,
            );
            state.selections.advance()?;
            state.selections.next_snapshot = id.0;
            let snapshot = SnapshotLease(Arc::new(Snapshot {
                id,
                session_id: state.selections.session_id.clone(),
                document: prepared.document,
                scope: prepared.scope,
                document_name: prepared.document_name,
                page_bytes: self.shared.config.selection.content_page_bytes,
                _permit: permit,
            }));
            state
                .selections
                .snapshots
                .retain(|_, weak| weak.strong_count() > 0);
            state
                .selections
                .snapshots
                .insert(id, Arc::downgrade(&snapshot.0));
            let previous = state.documents[index].selection.replace(snapshot);
            let removed = state
                .selections
                .cache_previous(previous, self.shared.config.selection.max_cached_snapshots);
            (state.capture_selection(), removed)
        };
        drop(removed);
        Ok(selection)
    }

    /// 明确清空活动选区；已经为空时幂等，不递增版本，也不改变查看器检查图层。
    pub fn clear_selection(
        &self,
        lease: &DocumentLease,
        expected: SelectionRevision,
    ) -> Result<UserSelection, SelectionError> {
        let (selection, removed) = {
            let mut state = lock(&self.shared.state);
            let index = state.validate_selection(lease, expected)?;
            if state.documents[index].selection.is_none() {
                return Ok(state.capture_selection());
            }
            state.selections.advance()?;
            let previous = state.documents[index].selection.take();
            let removed = state
                .selections
                .cache_previous(previous, self.shared.config.selection.max_cached_snapshots);
            (state.capture_selection(), removed)
        };
        drop(removed);
        Ok(selection)
    }

    /// 显式快照读取入口；缓存过期时失败，不回退到当前选择。长流程应先创建任务。
    pub fn selection_snapshot(
        &self,
        session: &SessionId,
        id: SnapshotId,
    ) -> Result<SnapshotLease, SelectionError> {
        let state = lock(&self.shared.state);
        state.ensure_running()?;
        state.selections.check_session(session)?;
        state.selections.snapshot(id)
    }

    /// 所有存活快照的即时计费采样；共享引用不重复计数。
    pub fn selection_resources(&self) -> SelectionAccounting {
        self.shared.selection_budget.usage()
    }
}

#[cfg(test)]
mod tests;
