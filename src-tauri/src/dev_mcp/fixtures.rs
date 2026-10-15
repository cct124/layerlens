//! 有界任务准备：串行调用真实共享服务，一次外层回执保留完整／部分结果。
//! 不绕过核心任务额度；重试同一请求只读原回执，部分失败不会隐式补做。

use crate::workspace::{WorkspaceHost, error};
use layerlens_core::{
    selection_contract::*,
    workspace_contract::{WorkspaceError, WorkspaceErrorCode as Code},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct PrepareTasks {
    pub snapshot_id: String,
    pub count: u32,
    pub name_prefix: String,
}
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(super) enum BatchStatus {
    Complete,
    Partial,
    Failed,
}
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct PreparedTasks {
    status: BatchStatus,
    requested: u32,
    task_ids: Vec<String>,
    /// 已完成数同时是首个失败的零基下标；其后的项没有执行。
    error: Option<WorkspaceError>,
}
pub(super) async fn prepare(
    host: &WorkspaceHost,
    session: String,
    request: PrepareTasks,
) -> Result<PreparedTasks, WorkspaceError> {
    if !(1..=128).contains(&request.count)
        || request.name_prefix.trim().is_empty()
        || request.name_prefix.len() > 240
    {
        return Err(error(
            Code::InvalidInput,
            "任务准备数量为 1–128，名称前缀为 1–240 UTF-8 字节且不能全为空白。",
        ));
    }
    request
        .snapshot_id
        .parse::<layerlens_core::documents::selection::SnapshotId>()
        .map_err(|e| error(Code::InvalidInput, e.to_string()))?;
    // 随机子请求空间只在外层回执接受后建立；重放不会创建新的空间或再次调用服务。
    let batch = uuid::Uuid::new_v4();
    let mut task_ids = Vec::with_capacity(request.count as usize);
    let mut failure = None;
    for index in 0..request.count {
        let result = host
            .operations
            .request(SelectionRequest {
                protocol_version: layerlens_core::IPC_PROTOCOL_VERSION,
                session_id: session.clone(),
                operation: SelectionOperation::CreateTask {
                    snapshot_id: request.snapshot_id.clone(),
                    request_id: format!("fixture:{batch}:{index}"),
                    name: format!("{} {}", request.name_prefix, index + 1),
                },
            })
            .await;
        match result {
            Ok(SelectionReply::Task { task, .. }) => task_ids.push(task.id),
            Ok(_) => {
                failure = Some(error(
                    Code::Internal,
                    "任务准备未收到任务回执，请核对核心状态。",
                ));
                break;
            }
            Err(error) => {
                failure = Some(error);
                break;
            }
        }
    }
    Ok(PreparedTasks {
        status: if failure.is_none() {
            BatchStatus::Complete
        } else if task_ids.is_empty() {
            BatchStatus::Failed
        } else {
            BatchStatus::Partial
        },
        requested: request.count,
        task_ids,
        error: failure,
    })
}
