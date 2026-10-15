//! IPC 适配层保留核心的输入、输出与错误契约，不复制业务规则。

use crate::workspace::{SingleOperation, WorkspaceHost, check_version, error};
use layerlens_core::workspace_contract::{
    LayerRequest, LayerResponse, PreviewRequest, WorkspaceError, WorkspaceErrorCode,
    WorkspaceRequest, WorkspaceSnapshot,
};
use layerlens_core::{AppInfo, AppInfoRequest, CommandError};
use std::sync::Arc;
use tauri_plugin_dialog::DialogExt;

/// 转发应用信息查询，由核心校验协议兼容性。
#[tauri::command]
pub(crate) fn get_app_info(request: AppInfoRequest) -> Result<AppInfo, CommandError> {
    layerlens_core::get_app_info(request)
}

/// 只进入有界选区后台；选择提交与内容响应均由同一个核心会话裁决。
#[tauri::command]
pub(crate) async fn selection_request(
    request: layerlens_core::selection_contract::SelectionRequest,
    host: tauri::State<'_, Arc<WorkspaceHost>>,
) -> Result<layerlens_core::selection_contract::SelectionReply, WorkspaceError> {
    #[cfg(feature = "dev-mcp")]
    let creating = matches!(
        request.operation,
        layerlens_core::selection_contract::SelectionOperation::CreateTask { .. }
    );
    let reply = host.operations.request(request).await?;
    #[cfg(feature = "dev-mcp")]
    {
        use crate::workspace::testing::Checkpoint;
        use layerlens_core::selection_contract::SelectionReply;
        let point = match &reply {
            SelectionReply::Tasks { .. } => Some(Checkpoint::DesktopTasks),
            SelectionReply::Task { .. } if creating => Some(Checkpoint::DesktopTaskCreated),
            SelectionReply::Content { .. } => Some(Checkpoint::DesktopContent),
            _ => None,
        };
        if let Some(point) = point {
            host.operations.testing.hit(point).await?;
        }
    }
    Ok(reply)
}

/// 预检和确认均进入同一有界操作后台；客户端不能传入自造的清理影响集合。
#[tauri::command]
pub(crate) async fn cleanup_request(
    request: layerlens_core::cleanup_contract::CleanupRequest,
    host: tauri::State<'_, Arc<WorkspaceHost>>,
) -> Result<layerlens_core::cleanup_contract::CleanupReply, WorkspaceError> {
    let reply = host.operations.cleanup(request).await?;
    #[cfg(feature = "dev-mcp")]
    if matches!(
        reply,
        layerlens_core::cleanup_contract::CleanupReply::Committed { .. }
    ) {
        host.operations
            .testing
            .hit(crate::workspace::testing::Checkpoint::DesktopCleanupCommitted)
            .await?;
    }
    Ok(reply)
}

#[tauri::command]
pub(crate) async fn workspace_action(
    request: WorkspaceRequest,
    host: tauri::State<'_, Arc<WorkspaceHost>>,
) -> Result<WorkspaceSnapshot, WorkspaceError> {
    check_version(request.protocol_version)?;
    #[cfg(feature = "dev-mcp")]
    let snapshot = matches!(
        request.action,
        layerlens_core::workspace_contract::WorkspaceAction::Snapshot {}
    );
    let reply = host.action(request.action).await?;
    #[cfg(feature = "dev-mcp")]
    if snapshot {
        host.operations
            .testing
            .hit(crate::workspace::testing::Checkpoint::DesktopSnapshot)
            .await?;
    }
    Ok(reply)
}

#[tauri::command]
pub(crate) async fn read_preview(
    request: PreviewRequest,
    host: tauri::State<'_, Arc<WorkspaceHost>>,
) -> Result<tauri::ipc::Response, WorkspaceError> {
    // 只复制已完成 PNG；二进制 IPC 不使用 JSON/base64，最多一个在途复制。
    host.read_image(request, |image| {
        Ok(tauri::ipc::Response::new(image.png().to_vec()))
    })
    .await
}

/// 固定修订后在阻塞池组装有界详情；一次只允许一个查询，避免复制大文字属性时无限并发。
#[tauri::command]
pub(crate) async fn read_layers(
    request: LayerRequest,
    host: tauri::State<'_, Arc<WorkspaceHost>>,
) -> Result<LayerResponse, WorkspaceError> {
    host.read_layers(request).await
}

#[tauri::command]
pub(crate) async fn choose_psd_files<R: tauri::Runtime>(
    request: AppInfoRequest,
    window: tauri::WebviewWindow<R>,
    host: tauri::State<'_, Arc<WorkspaceHost>>,
) -> Result<Vec<String>, WorkspaceError> {
    check_version(request.protocol_version)?;
    let guard = SingleOperation::acquire(&host.picking)?;
    let (sender, receiver) = tokio::sync::oneshot::channel();
    window
        .dialog()
        .file()
        .set_parent(&window)
        .add_filter("Photoshop", &["psd"])
        .pick_files(move |files| {
            let _guard = guard;
            let _ = sender.send(files);
        });
    let files = receiver
        .await
        .map_err(|_| error(WorkspaceErrorCode::Internal, "文件选择器未返回结果。"))?;
    files
        .unwrap_or_default()
        .into_iter()
        .map(|file| {
            let path = file.into_path().map_err(|e| {
                error(
                    WorkspaceErrorCode::InvalidInput,
                    format!("无法读取所选路径：{e}"),
                )
            })?;
            path.into_os_string().into_string().map_err(|_| {
                error(
                    WorkspaceErrorCode::InvalidInput,
                    "所选路径无法用 Unicode 表示。",
                )
            })
        })
        .collect()
}
