//! 有界任务与请求幂等索引；终态不再持有快照，记录保留至会话结束。

use std::time::SystemTime;

use super::{
    DesignTask, SelectionError, SelectionHandle, SessionId, SnapshotId, SnapshotLease, TaskId,
    TaskPage, TaskStatus, model::limit,
};
use crate::documents::{DocumentError, model::lock};

pub(super) struct TaskEntry {
    record: DesignTask,
    pub(super) snapshot: Option<SnapshotLease>,
}

impl SelectionHandle {
    /// 以客户端请求 ID 固定快照；同 ID 同参数返回原记录（包括已释放终态），不同参数冲突。
    /// 名称允许重复；请求 ID 为 1–128 字节，名称为 1–256 字节且不能全为空白。
    ///
    /// # Errors
    /// 会话不符、快照过期、无有效可见目标或记录满载时不创建任务／幂等记录。
    /// 释放任务不删除幂等记录；满载须结束会话，不能静默淘汰重试结果。
    pub fn create_task(
        &self,
        session: &SessionId,
        request_id: &str,
        snapshot_id: SnapshotId,
        name: &str,
    ) -> Result<DesignTask, SelectionError> {
        if request_id.trim().is_empty()
            || request_id.len() > 128
            || name.trim().is_empty()
            || name.len() > 256
        {
            return Err(SelectionError::InvalidInput("请求 ID 或任务名称为空／超长"));
        }
        // 可能是最后一个强引用，失败时必须晚于状态锁析构。
        let snapshot;
        let mut state = lock(&self.shared.state);
        state.ensure_running()?;
        let selections = &mut state.selections;
        selections.check_session(session)?;
        if let Some(id) = selections.requests.get(request_id) {
            // 请求索引与记录同锁插入，且会话内不删除终态记录。
            let task = &selections.tasks[id].record;
            if task.snapshot_id != snapshot_id || task.name != name {
                return Err(SelectionError::RequestConflict);
            }
            return Ok(task.clone());
        }
        if selections.tasks.len() >= self.shared.config.selection.max_task_records {
            return Err(limit(
                "设计任务记录",
                self.shared.config.selection.max_task_records as u64,
            ));
        }
        snapshot = selections.snapshot(snapshot_id)?;
        if snapshot.scope().target_layer_ids.is_empty() {
            return Err(SelectionError::EmptyTargets);
        }
        let id = TaskId(
            selections
                .next_task
                .checked_add(1)
                .ok_or(DocumentError::IdExhausted)?,
        );
        let record = DesignTask {
            scope: snapshot.brief(),
            id,
            snapshot_id,
            name: name.to_owned(),
            created_at: SystemTime::now(),
            status: TaskStatus::Active,
        };
        selections.next_task = id.0;
        selections.tasks.insert(
            id,
            TaskEntry {
                record: record.clone(),
                snapshot: Some(snapshot),
            },
        );
        selections.requests.insert(request_id.to_owned(), id);
        Ok(record)
    }

    /// 每页 1–32 条，按 ID 升序；游标必须属于此会话的既有任务。
    pub fn list_tasks(
        &self,
        session: &SessionId,
        after: Option<TaskId>,
        limit: u32,
    ) -> Result<TaskPage, SelectionError> {
        if !(1..=32).contains(&limit) {
            return Err(SelectionError::InvalidInput("任务页条数应为 1–32"));
        }
        let state = lock(&self.shared.state);
        state.ensure_running()?;
        state.selections.check_session(session)?;
        let tasks = &state.selections.tasks;
        if after.is_some_and(|id| !tasks.contains_key(&id)) {
            return Err(SelectionError::TaskNotFound);
        }
        let mut iter = tasks
            .iter()
            .filter(|(id, _)| after.is_none_or(|after| **id > after));
        let records: Vec<_> = iter
            .by_ref()
            .take(limit as usize)
            .map(|(_, task)| task.record.clone())
            .collect();
        let next_after = if iter.next().is_some() {
            records.last().map(|task| task.id)
        } else {
            None
        };
        Ok(TaskPage {
            tasks: records,
            next_after,
            total: tasks.len(),
            capacity: self.shared.config.selection.max_task_records,
        })
    }

    /// 查询小型任务记录；已释放任务仍可核对结果，不隐式发起设计读取。
    pub fn design_task(
        &self,
        session: &SessionId,
        id: TaskId,
    ) -> Result<DesignTask, SelectionError> {
        let state = lock(&self.shared.state);
        state.ensure_running()?;
        state.selections.check_session(session)?;
        state
            .selections
            .tasks
            .get(&id)
            .map(|task| task.record.clone())
            .ok_or(SelectionError::TaskNotFound)
    }

    /// 在请求开始时固定任务引用；之后的释放不影响本次读取，新请求则被拒绝。
    pub fn task_snapshot(
        &self,
        session: &SessionId,
        id: TaskId,
    ) -> Result<SnapshotLease, SelectionError> {
        let state = lock(&self.shared.state);
        state.ensure_running()?;
        state.selections.check_session(session)?;
        let task = state
            .selections
            .tasks
            .get(&id)
            .ok_or(SelectionError::TaskNotFound)?;
        task.snapshot.clone().ok_or(SelectionError::TaskReleased)
    }

    /// 幂等释放；未知任务明确报错。最后一个源修订引用在状态锁外释放。
    pub fn release_task(
        &self,
        session: &SessionId,
        id: TaskId,
    ) -> Result<DesignTask, SelectionError> {
        let (record, removed) = {
            let mut state = lock(&self.shared.state);
            state.ensure_running()?;
            state.selections.check_session(session)?;
            let task = state
                .selections
                .tasks
                .get_mut(&id)
                .ok_or(SelectionError::TaskNotFound)?;
            task.record.status = TaskStatus::Released;
            (task.record.clone(), task.snapshot.take())
        };
        drop(removed);
        Ok(record)
    }
}
