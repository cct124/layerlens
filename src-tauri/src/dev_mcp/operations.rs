//! 开发工具只增加受控文件边界和观测条件，所有业务仍进入桌面共享入口。

use super::{config::Instance, model::*};
use crate::workspace::{WorkspaceHost, check_version, error};
use layerlens_core::workspace_contract::*;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

pub(super) async fn workspace(
    host: Arc<WorkspaceHost>,
    instance: Arc<Instance>,
    mut request: WorkspaceRequest,
) -> Result<WorkspaceResult, WorkspaceError> {
    check_version(request.protocol_version)?;
    let snapshot_only = matches!(request.action, WorkspaceAction::Snapshot {});
    let path = match &request.action {
        WorkspaceAction::Open { path } => Some(path.clone()),
        WorkspaceAction::Reload { document_id } => {
            let current = host.action(WorkspaceAction::Snapshot {}).await?;
            Some(
                current
                    .documents
                    .iter()
                    .find(|d| d.id == *document_id)
                    .ok_or_else(|| {
                        error(WorkspaceErrorCode::NotFound, "重载目标不在当前测试实例中。")
                    })?
                    .path
                    .clone(),
            )
        }
        _ => None,
    };
    if let Some(path) = path {
        let canonical = tauri::async_runtime::spawn_blocking(move || instance.sample(&path))
            .await
            .map_err(|e| {
                error(
                    WorkspaceErrorCode::Internal,
                    format!("样本边界检查失败：{e}"),
                )
            })?
            .map_err(|e| {
                error(
                    WorkspaceErrorCode::InvalidInput,
                    format!("样本路径被拒绝：{e}"),
                )
            })?;
        if let WorkspaceAction::Open { path } = &mut request.action {
            *path = canonical.into_os_string().into_string().map_err(|_| {
                error(
                    WorkspaceErrorCode::InvalidInput,
                    "样本路径无法用 Unicode 表示。",
                )
            })?;
        }
    }
    let snapshot = host.action(request.action).await?;
    Ok(if snapshot_only {
        WorkspaceResult::Snapshot {
            snapshot: Box::new(snapshot),
        }
    } else {
        WorkspaceResult::Accepted {
            sequence: snapshot.sequence,
            active_document_id: snapshot.active_document_id,
            job_ids: snapshot.jobs.into_iter().map(|j| j.id).collect(),
        }
    })
}

fn number(value: &str) -> Result<u64, WorkspaceError> {
    let n = value.parse::<u64>().map_err(|_| {
        error(
            WorkspaceErrorCode::InvalidInput,
            "版本和 ID 必须是无损十进制字符串。",
        )
    })?;
    if n.to_string() != value {
        return Err(error(
            WorkspaceErrorCode::InvalidInput,
            "版本和 ID 不是规范十进制表示。",
        ));
    }
    Ok(n)
}

impl WaitCondition {
    pub(super) fn validate(&self) -> Result<(), WorkspaceError> {
        if let Self::FrontendImageReady { at_least, .. } = self {
            number(at_least)?;
        }
        match self {
            Self::Sequence { at_least } | Self::Cleanup { at_least } => {
                number(at_least)?;
            }
            Self::DocumentRevision {
                document_id,
                revision,
            }
            | Self::PreviewReady {
                document_id,
                revision,
            }
            | Self::FrontendImageReady {
                document_id,
                revision,
                ..
            } => {
                number(document_id)?;
                number(revision)?;
            }
            Self::DocumentCount { count } if *count > 8 => {
                return Err(error(
                    WorkspaceErrorCode::InvalidInput,
                    "文档数条件超过当前默认修订上限 8。",
                ));
            }
            _ => {}
        }
        Ok(())
    }
    fn matches(&self, state: &WorkspaceSnapshot) -> Result<bool, WorkspaceError> {
        Ok(match self {
            Self::FrontendImageReady { .. } => false,
            Self::Sequence { at_least } => number(&state.sequence)? >= number(at_least)?,
            Self::Cleanup { at_least } => number(&state.cleanup.revision)? >= number(at_least)?,
            Self::DocumentCount { count } => state.documents.len() == *count as usize,
            Self::DocumentRevision {
                document_id,
                revision,
            } => state
                .documents
                .iter()
                .any(|d| d.id == *document_id && d.revision == *revision),
            Self::Idle {} => state.jobs.is_empty(),
            Self::ResourcesReleased {} => {
                state.resources.source_bytes == "0"
                    && state.resources.output_bytes == "0"
                    && state.resources.decoded_bytes == "0"
                    && state.resources.cache_bytes == "0"
            }
            Self::PreviewReady {
                document_id,
                revision,
            } => state.preview.as_ref().is_some_and(|p| {
                p.document_id == *document_id
                    && p.revision == *revision
                    && matches!(p.state, WorkspacePreviewState::Ready { .. })
            }),
        })
    }
}

pub(super) async fn wait(
    host: &WorkspaceHost,
    observations: &super::ui::Observations,
    request: WaitRequest,
    stopping: &CancellationToken,
) -> Result<WaitResult, WorkspaceError> {
    if request.timeout_ms == 0 || request.timeout_ms > 30_000 {
        return Err(error(
            WorkspaceErrorCode::InvalidInput,
            "等待上限须为 1–30000 ms。",
        ));
    }
    request.condition.validate()?;
    // 先订阅再采样，避免检查与订阅之间丢失状态变化；等待不占工作区邮箱。
    let mut receiver = host.observe();
    let mut frontend = observations.subscribe();
    let mut last = host.action(WorkspaceAction::Snapshot {}).await?;
    let deadline = tokio::time::Instant::now()
        + std::time::Duration::from_millis(u64::from(request.timeout_ms));
    loop {
        let observed = receiver.borrow_and_update().clone();
        if let Some(observed) = observed
            && number(&observed.sequence)? >= number(&last.sequence)?
        {
            last = observed;
        }
        let rendered = frontend.borrow_and_update().clone();
        let satisfied = match &request.condition {
            WaitCondition::FrontendImageReady {
                document_id,
                revision,
                at_least,
            } => rendered.as_ref().is_some_and(|view| {
                view.active_document
                    .as_ref()
                    .is_some_and(|d| &d.id == document_id && &d.revision == revision)
                    && view
                        .core_sequence
                        .as_ref()
                        .is_some_and(|sequence| number(sequence).ok() >= number(at_least).ok())
                    && view
                        .canvas
                        .as_ref()
                        .is_some_and(|c| c.image_decoded && c.paint_prepared)
            }),
            _ => request.condition.matches(&last)?,
        };
        if satisfied {
            return Ok(WaitResult {
                satisfied: true,
                timed_out: false,
                condition: request.condition,
                last_state: last,
            });
        }
        tokio::select! {
            _ = stopping.cancelled() => return Err(error(WorkspaceErrorCode::ShuttingDown, "测试实例正在退出，等待已终止。")),
            _ = tokio::time::sleep_until(deadline) => return Ok(WaitResult { satisfied: false, timed_out: true, condition: request.condition, last_state: last }),
            changed = receiver.changed() => changed.map_err(|_| error(WorkspaceErrorCode::ShuttingDown, "工作区观测通道已断开。"))?,
            changed = frontend.changed(), if matches!(request.condition, WaitCondition::FrontendImageReady { .. }) => changed.map_err(|_| error(WorkspaceErrorCode::ShuttingDown, "前端观测通道已断开。"))?,
        }
    }
}
