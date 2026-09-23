//! 可发送的选区服务句柄只共享注册表，不拥有加载线程或另建核心。
//! 应用仍由 DocumentService 管理退出；退出后所有句柄入口拒绝新操作。

use super::*;

/// 供有界后台执行选区／任务工作；丢弃句柄不发起整个工作区退出。
#[derive(Clone)]
pub struct SelectionHandle {
    pub(super) shared: Arc<state::Shared>,
}

impl SelectionHandle {
    /// 固定显式文档修订，同时核对会话；不会切换活动标签。
    pub fn document(
        &self,
        session: &SessionId,
        reference: DocumentReference,
    ) -> Result<DocumentLease, SelectionError> {
        let state = lock(&self.shared.state);
        state.ensure_running()?;
        state.selections.check_session(session)?;
        let entry = state
            .documents
            .iter()
            .find(|entry| entry.revision.document_id == reference.document_id)
            .ok_or(DocumentError::NotFound(reference.document_id))?;
        if entry.revision.id != reference.revision_id {
            return Err(DocumentError::StaleRevision.into());
        }
        Ok(DocumentLease(Arc::clone(&entry.revision)))
    }
}

// 保留 M2-01 已使用的 Rust 入口；规则只有 SelectionHandle 一份实现。
impl DocumentService {
    /// 借出同一状态的后台句柄；调用方必须自行限制并发并在退出时等待其工作。
    pub fn selection_handle(&self) -> SelectionHandle {
        SelectionHandle {
            shared: Arc::clone(&self.shared),
        }
    }
    /// 原子取得活动选择及其固定引用，详见 SelectionHandle::user_selection。
    pub fn user_selection(&self) -> Result<UserSelection, SelectionError> {
        self.selection_handle().user_selection()
    }
    /// 锁外规范化；详见 SelectionHandle::prepare_selection 的版本与预算契约。
    pub fn prepare_selection(
        &self,
        lease: &DocumentLease,
        expected: SelectionRevision,
        input: SelectionInput,
    ) -> Result<PreparedSelection, SelectionError> {
        self.selection_handle()
            .prepare_selection(lease, expected, input)
    }
    /// 校验版本后原子发布；同范围幂等，失败保留旧选择。
    pub fn commit_selection(
        &self,
        prepared: PreparedSelection,
    ) -> Result<UserSelection, SelectionError> {
        self.selection_handle().commit_selection(prepared)
    }
    /// 明确清空当前选区，不改变检查焦点或任务。
    pub fn clear_selection(
        &self,
        lease: &DocumentLease,
        expected: SelectionRevision,
    ) -> Result<UserSelection, SelectionError> {
        self.selection_handle().clear_selection(lease, expected)
    }
    /// 固定显式快照，过期时拒绝而不是退回当前选择。
    pub fn selection_snapshot(
        &self,
        session: &SessionId,
        id: SnapshotId,
    ) -> Result<SnapshotLease, SelectionError> {
        self.selection_handle().selection_snapshot(session, id)
    }
    /// 读取共享范围账本。
    pub fn selection_resources(&self) -> SelectionAccounting {
        self.selection_handle().selection_resources()
    }
    /// 幂等固定任务；详见 SelectionHandle::create_task。
    pub fn create_task(
        &self,
        session: &SessionId,
        request: &str,
        snapshot: SnapshotId,
        name: &str,
    ) -> Result<DesignTask, SelectionError> {
        self.selection_handle()
            .create_task(session, request, snapshot, name)
    }
    /// 查询包括已释放／失效终态在内的小型记录。
    pub fn design_task(
        &self,
        session: &SessionId,
        id: TaskId,
    ) -> Result<DesignTask, SelectionError> {
        self.selection_handle().design_task(session, id)
    }
    /// 固定任务读取引用，已释放／失效任务不能发起新读取。
    pub fn task_snapshot(
        &self,
        session: &SessionId,
        id: TaskId,
    ) -> Result<SnapshotLease, SelectionError> {
        self.selection_handle().task_snapshot(session, id)
    }
    /// 幂等释放；既有在途读取仍可完成。
    pub fn release_task(
        &self,
        session: &SessionId,
        id: TaskId,
    ) -> Result<DesignTask, SelectionError> {
        self.selection_handle().release_task(session, id)
    }
}
