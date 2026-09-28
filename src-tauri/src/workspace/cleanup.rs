//! 清理计划仅由单个有界操作后台保管。未确认计划可过期；成功回执在会话内保留且不淘汰。
use super::{budget::check_size, check_version, domain_error, error, selection::selection_error};
use layerlens_core::{
    cleanup_contract::*,
    documents::{
        CleanupHandle, CleanupPlan,
        selection::{DocumentReference, SelectionHandle, SessionId},
    },
    workspace_contract::{WorkspaceError, WorkspaceErrorCode},
};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

pub(super) const RESPONSE_BYTES: usize = 256 * 1024;
const REQUEST_BYTES: usize = 4096;
const RECEIPT_BYTES: usize = 8 * 1024;
const PLAN_LIMIT: usize = 4;
const RECEIPT_LIMIT: usize = 128;
const PLAN_TTL: Duration = Duration::from_secs(300);

struct Pending {
    plan: CleanupPlan,
    created: Instant,
}
#[derive(Default)]
pub(super) struct Plans {
    next_id: u64,
    pending: BTreeMap<String, Pending>,
    completed: BTreeMap<String, CleanupReceiptDto>,
}

pub(super) fn validate_request(request: &CleanupRequest) -> Result<(), WorkspaceError> {
    check_version(request.protocol_version)?;
    check_size(request, REQUEST_BYTES)?;
    let valid_id = |id: &str| {
        !id.is_empty()
            && !id.starts_with('0')
            && id.len() <= 20
            && id.bytes().all(|b| b.is_ascii_digit())
            && id.parse::<u64>().is_ok()
    };
    if let CleanupOperation::Confirm { plan_id } | CleanupOperation::Cancel { plan_id } =
        &request.operation
        && !valid_id(plan_id)
    {
        return Err(error(
            WorkspaceErrorCode::InvalidInput,
            "清理计划标识无效。",
        ));
    }
    Ok(())
}

impl Plans {
    pub(super) fn expire(&mut self, now: Instant) {
        self.pending
            .retain(|_, entry| now.saturating_duration_since(entry.created) < PLAN_TTL);
    }

    pub(super) fn execute(
        &mut self,
        selection: &SelectionHandle,
        cleanup: &CleanupHandle,
        request: CleanupRequest,
        now: Instant,
        response_bytes: usize,
    ) -> Result<CleanupReply, WorkspaceError> {
        validate_request(&request)?;
        let session: SessionId = request.session_id.parse().map_err(selection_error)?;
        if selection
            .user_selection()
            .map_err(selection_error)?
            .session_id
            != session
        {
            return Err(error(
                WorkspaceErrorCode::ForeignSession,
                "清理计划不属于当前会话。",
            ));
        }
        // 先预留最坏回执（128 个 u64 任务 ID），确认后不再执行可能失败的序列化准入。
        if response_bytes < RECEIPT_BYTES {
            return Err(error(
                WorkspaceErrorCode::ResourceLimit,
                "无法预留清理回执，未执行变更。",
            ));
        }
        self.expire(now);
        let session_id = session.as_str().to_owned();
        match request.operation {
            CleanupOperation::Prepare { target } => {
                if self.pending.len() >= PLAN_LIMIT
                    || self.completed.len() + self.pending.len() >= RECEIPT_LIMIT
                {
                    return Err(error(
                        WorkspaceErrorCode::ResourceLimit,
                        "清理计划或会话回执已满；取消未确认计划，或结束会话后重试。",
                    ));
                }
                let (document, expected_revision) = match target {
                    CleanupTarget::Document {
                        document_id,
                        document_revision,
                    } => {
                        // 租约在本后台释放，不交给观察线程或计划保管。
                        let reference = DocumentReference::parse(&document_id, &document_revision)
                            .map_err(selection_error)?;
                        let lease = selection
                            .document(&session, reference)
                            .map_err(selection_error)?;
                        (lease.document_id(), Some(lease.revision_id()))
                    }
                    CleanupTarget::Task { task_id } => (
                        selection
                            .design_task(&session, task_id.parse().map_err(selection_error)?)
                            .map_err(selection_error)?
                            .scope
                            .document_id,
                        None,
                    ),
                };
                let plan = cleanup
                    .prepare_cleanup(document)
                    .map_err(|e| domain_error(&e))?;
                let id = self.next_id.checked_add(1).ok_or_else(|| {
                    error(WorkspaceErrorCode::ResourceLimit, "清理计划标识已耗尽。")
                })?;
                let plan_id = id.to_string();
                let impact = plan.impact();
                if expected_revision.is_some() && impact.open_revision != expected_revision {
                    return Err(error(
                        WorkspaceErrorCode::StaleRevision,
                        "目标文档已变化，请重新发起清理。",
                    ));
                }
                // 桌面使用默认核心预算；即使日后放宽配置也不能突破当前协议的集合上限。
                if impact.tasks.len() > 128
                    || impact.live_revisions.len() > 8
                    || impact.load_jobs.len() > 4
                    || impact.unresolved_load_jobs.len() > 4
                    || impact.preview_jobs.len() > 4
                {
                    return Err(error(
                        WorkspaceErrorCode::ResourceLimit,
                        "清理影响超出桌面展示预算，未创建计划。",
                    ));
                }
                let dto = CleanupPlanDto {
                    plan_id: plan_id.clone(),
                    document_id: document.get().to_string(),
                    document_name: impact.document_name.clone(),
                    open_revision: impact.open_revision.map(|id| id.get().to_string()),
                    selection_id: impact.selection.map(|id| id.get().to_string()),
                    live_revisions: impact
                        .live_revisions
                        .iter()
                        .map(|id| id.get().to_string())
                        .collect(),
                    tasks: impact
                        .tasks
                        .iter()
                        .map(|task| CleanupTaskDto {
                            id: task.id.get().to_string(),
                            name: task.name.clone(),
                            document_revision: task.scope.revision_id.get().to_string(),
                        })
                        .collect(),
                    load_jobs: impact
                        .load_jobs
                        .iter()
                        .map(|id| id.get().to_string())
                        .collect(),
                    unresolved_load_jobs: impact
                        .unresolved_load_jobs
                        .iter()
                        .map(|id| id.get().to_string())
                        .collect(),
                    preview_jobs: impact
                        .preview_jobs
                        .iter()
                        .map(|id| id.get().to_string())
                        .collect(),
                    expires_in_seconds: PLAN_TTL.as_secs() as u32,
                };
                let reply = CleanupReply::Prepared {
                    session_id,
                    plan: dto,
                };
                check_size(&reply, response_bytes)?;
                self.next_id = id;
                self.pending.insert(plan_id, Pending { plan, created: now });
                Ok(reply)
            }
            CleanupOperation::Confirm { plan_id } => {
                if let Some(receipt) = self.completed.get(&plan_id) {
                    return Ok(CleanupReply::Committed {
                        session_id,
                        receipt: receipt.clone(),
                    });
                }
                let entry = self.pending.get(&plan_id).ok_or_else(|| {
                    error(
                        WorkspaceErrorCode::CleanupPlanExpired,
                        "清理计划已取消、过期或不存在，请重新预检。",
                    )
                })?;
                let result = cleanup
                    .commit_cleanup(&entry.plan)
                    .map_err(|e| domain_error(&e));
                let result = match result {
                    Ok(result) => result,
                    Err(error) => {
                        self.pending.remove(&plan_id);
                        return Err(error);
                    }
                };
                let receipt = CleanupReceiptDto {
                    plan_id: plan_id.clone(),
                    document_id: result.document_id.get().to_string(),
                    revision: result.revision.to_string(),
                    invalidated_task_ids: entry
                        .plan
                        .impact()
                        .tasks
                        .iter()
                        .map(|task| task.id.get().to_string())
                        .collect(),
                };
                self.pending.remove(&plan_id);
                self.completed.insert(plan_id, receipt.clone());
                Ok(CleanupReply::Committed {
                    session_id,
                    receipt,
                })
            }
            CleanupOperation::Cancel { plan_id } => {
                // 确认响应丢失后取消也必须返回真实成功回执，不能伪装为未执行。
                if let Some(receipt) = self.completed.get(&plan_id) {
                    return Ok(CleanupReply::Committed {
                        session_id,
                        receipt: receipt.clone(),
                    });
                }
                self.pending.remove(&plan_id);
                Ok(CleanupReply::Cancelled {
                    session_id,
                    plan_id,
                })
            }
        }
    }
}

/// 检查点不跨耗时计算持锁；检查后发生的清理由前端文档修订和终态投影拒绝晚到结果。
pub(crate) fn validate_publication(
    handle: &CleanupHandle,
    document: layerlens_core::documents::DocumentId,
    revision: layerlens_core::documents::RevisionId,
) -> Result<(), WorkspaceError> {
    handle
        .validate_open_revision(document, revision)
        .map_err(|e| domain_error(&e))
}

#[cfg(test)]
mod tests;
