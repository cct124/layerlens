//! 选区／任务适配。唯一核心句柄在有界后台使用；回执与按需内容分离。

mod dto;
mod worker;
pub(crate) use dto::summary;
pub(super) use worker::{SelectionClient, SelectionWorker};

use super::{domain_error, error};
use layerlens_core::{
    documents::selection::*, psd::LayerId, selection_contract::*, workspace_contract::*,
};

const RESPONSE_BYTES: usize = 256 * 1024;
// 变更回执没有不定长集合：两项字符串各 <=256 UTF-8 字节，即使全部 JSON 转义，
// 加上固定字段、u64/u32 的最大位数也小于 8 KiB。新增字段须更新最坏值回归。
const RECEIPT_BYTES: usize = 8 * 1024;
const REQUEST_BYTES: usize = 64 * 1024;

fn selection_error(value: SelectionError) -> WorkspaceError {
    let code = match &value {
        SelectionError::Document(value) => return domain_error(value),
        SelectionError::InvalidInput(_) => WorkspaceErrorCode::InvalidInput,
        SelectionError::ForeignSession => WorkspaceErrorCode::ForeignSession,
        SelectionError::StaleSelection => WorkspaceErrorCode::StaleSelection,
        SelectionError::InactiveDocument => WorkspaceErrorCode::InactiveDocument,
        SelectionError::SnapshotExpired => WorkspaceErrorCode::SnapshotExpired,
        SelectionError::EmptyTargets => WorkspaceErrorCode::EmptyTargets,
        SelectionError::TaskNotFound => WorkspaceErrorCode::NotFound,
        SelectionError::TaskReleased => WorkspaceErrorCode::TaskReleased,
        SelectionError::TaskInvalidated => WorkspaceErrorCode::TaskInvalidated,
        SelectionError::RequestConflict => WorkspaceErrorCode::RequestConflict,
    };
    error(code, value.to_string())
}

fn check_size(value: &impl serde::Serialize, limit: usize) -> Result<(), WorkspaceError> {
    struct Counter {
        written: usize,
        limit: usize,
    }
    impl std::io::Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if bytes.len() > self.limit.saturating_sub(self.written) {
                return Err(std::io::Error::other("响应或请求超过字节预算"));
            }
            self.written += bytes.len();
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    serde_json::to_writer(Counter { written: 0, limit }, value).map_err(|_| {
        error(
            WorkspaceErrorCode::ResourceLimit,
            "选区请求或完整响应超过预算。",
        )
    })
}

fn validate_request(request: &SelectionRequest) -> Result<(), WorkspaceError> {
    super::check_version(request.protocol_version)?;
    check_size(request, REQUEST_BYTES)?;
    if let SelectionOperation::Layers { layer_ids, .. } = &request.operation
        && (layer_ids.is_empty() || layer_ids.len() > 4096)
    {
        return Err(error(
            WorkspaceErrorCode::InvalidInput,
            "选择 1–4096 个图层；清空须使用明确操作。",
        ));
    }
    Ok(())
}

fn execute(
    handle: &SelectionHandle,
    request: SelectionRequest,
    response_bytes: usize,
) -> Result<SelectionReply, WorkspaceError> {
    validate_request(&request)?;
    let session: SessionId = request.session_id.parse().map_err(selection_error)?;
    // 在任何发布、创建或释放之前保留完整回执额度；查询没有副作用。
    let mutation = !matches!(
        &request.operation,
        SelectionOperation::Content { .. } | SelectionOperation::Tasks { .. }
    );
    if mutation && response_bytes < RECEIPT_BYTES {
        return Err(error(
            WorkspaceErrorCode::ResourceLimit,
            "无法预留操作回执空间，未执行变更。",
        ));
    }
    let result = (|| -> Result<SelectionReply, SelectionError> {
        let receipt = |selection: UserSelection| SelectionReply::Committed {
            session_id: selection.session_id.as_str().to_owned(),
            selection_revision: selection.revision.get().to_string(),
            snapshot_id: selection.snapshot.map(|s| s.id().get().to_string()),
        };
        let reply = match request.operation {
            SelectionOperation::Layers {
                document_id,
                document_revision,
                expected_revision,
                layer_ids,
            } => {
                let lease = handle.document(
                    &session,
                    DocumentReference::parse(&document_id, &document_revision)?,
                )?;
                let refs = layer_ids
                    .into_iter()
                    .map(|id| LayerReference {
                        document_id: lease.document_id(),
                        revision_id: lease.revision_id(),
                        layer_id: LayerId(id),
                    })
                    .collect();
                let prepared = handle.prepare_selection(
                    &lease,
                    expected_revision.parse()?,
                    SelectionInput::Layers(refs),
                )?;
                receipt(handle.commit_selection(prepared)?)
            }
            SelectionOperation::Region {
                document_id,
                document_revision,
                expected_revision,
                bounds,
            } => {
                let lease = handle.document(
                    &session,
                    DocumentReference::parse(&document_id, &document_revision)?,
                )?;
                let prepared = handle.prepare_selection(
                    &lease,
                    expected_revision.parse()?,
                    SelectionInput::Region(bounds),
                )?;
                receipt(handle.commit_selection(prepared)?)
            }
            SelectionOperation::Clear {
                document_id,
                document_revision,
                expected_revision,
            } => {
                let lease = handle.document(
                    &session,
                    DocumentReference::parse(&document_id, &document_revision)?,
                )?;
                receipt(handle.clear_selection(&lease, expected_revision.parse()?)?)
            }
            SelectionOperation::CreateTask {
                snapshot_id,
                request_id,
                name,
            } => SelectionReply::Task {
                session_id: session.as_str().to_owned(),
                task: dto::task(handle.create_task(
                    &session,
                    &request_id,
                    snapshot_id.parse()?,
                    &name,
                )?),
            },
            SelectionOperation::ReleaseTask { task_id } => SelectionReply::Task {
                session_id: session.as_str().to_owned(),
                task: dto::task(handle.release_task(&session, task_id.parse()?)?),
            },
            SelectionOperation::Tasks { after, limit } => {
                let page =
                    handle.list_tasks(&session, after.map(|id| id.parse()).transpose()?, limit)?;
                SelectionReply::Tasks {
                    page: TaskPageDto {
                        session_id: session.as_str().to_owned(),
                        tasks: page.tasks.into_iter().map(dto::task).collect(),
                        next_after: page.next_after.map(|id| id.get().to_string()),
                        total: page
                            .total
                            .try_into()
                            .map_err(|_| SelectionError::InvalidInput("任务总数超出桌面范围"))?,
                        capacity: page
                            .capacity
                            .try_into()
                            .map_err(|_| SelectionError::InvalidInput("任务额度超出桌面范围"))?,
                    },
                }
            }
            SelectionOperation::Content {
                target,
                cursor,
                limit,
            } => {
                let response_target = target.clone();
                let snapshot = match target {
                    SelectionTarget::Snapshot { snapshot_id } => {
                        handle.selection_snapshot(&session, snapshot_id.parse()?)?
                    }
                    SelectionTarget::Task { task_id } => {
                        handle.task_snapshot(&session, task_id.parse()?)?
                    }
                };
                let cursor = cursor
                    .map(|c| {
                        Ok::<_, SelectionError>(ContentCursor::from_parts(
                            c.session_id.parse()?,
                            c.snapshot_id.parse()?,
                            c.record,
                            c.text_start,
                        ))
                    })
                    .transpose()?;
                // DTO 保持核心页字段表示，额外预留 1 KiB 供 kind/page 外层，最后检查整个回复。
                let page = snapshot.content_page_with_budget(
                    cursor.as_ref(),
                    limit,
                    response_bytes.saturating_sub(1024) as u64,
                )?;
                SelectionReply::Content {
                    target: response_target,
                    page: dto::content(page),
                }
            }
        };
        Ok(reply)
    })()
    .map_err(selection_error)?;
    check_size(&result, response_bytes)?;
    Ok(result)
}

#[cfg(test)]
mod tests;
