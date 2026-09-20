//! IPC 适配层保留核心的输入、输出与错误契约，不复制业务规则。

use crate::workspace::{WorkspaceHost, check_version, error};
use layerlens_core::workspace_contract::{
    LayerQuery, LayerRequest, LayerResponse, LayerResult, PreviewRequest, WorkspaceError,
    WorkspaceErrorCode, WorkspaceRequest, WorkspaceSnapshot,
};
use layerlens_core::{AppInfo, AppInfoRequest, CommandError};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tauri_plugin_dialog::DialogExt;

/// 转发应用信息查询，由核心校验协议兼容性。
#[tauri::command]
pub(crate) fn get_app_info(request: AppInfoRequest) -> Result<AppInfo, CommandError> {
    layerlens_core::get_app_info(request)
}

struct SingleOperation(Arc<AtomicBool>);

/// 只进入有界选区后台；选择提交与内容响应均由同一个核心会话裁决。
#[tauri::command]
pub(crate) async fn selection_request(
    request: layerlens_core::selection_contract::SelectionRequest,
    host: tauri::State<'_, WorkspaceHost>,
) -> Result<layerlens_core::selection_contract::SelectionReply, WorkspaceError> {
    host.selection.request(request).await
}

impl SingleOperation {
    fn acquire(flag: &Arc<AtomicBool>) -> Result<Self, WorkspaceError> {
        flag.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| error(WorkspaceErrorCode::Busy, "上一项操作尚未结束，请稍后重试。"))?;
        Ok(Self(flag.clone()))
    }
}

impl Drop for SingleOperation {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

#[tauri::command]
pub(crate) async fn workspace_action(
    request: WorkspaceRequest,
    host: tauri::State<'_, WorkspaceHost>,
) -> Result<WorkspaceSnapshot, WorkspaceError> {
    check_version(request.protocol_version)?;
    host.action(request.action).await
}

#[tauri::command]
pub(crate) async fn read_preview(
    request: PreviewRequest,
    host: tauri::State<'_, WorkspaceHost>,
) -> Result<tauri::ipc::Response, WorkspaceError> {
    check_version(request.protocol_version)?;
    let guard = SingleOperation::acquire(&host.image_reading)?;
    let image = host.image(request).await?;
    // 只复制已完成 PNG；二进制 IPC 不使用 JSON/base64，最多一个在途复制。
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = guard;
        tauri::ipc::Response::new(image.png().to_vec())
    })
    .await
    .map_err(|e| error(WorkspaceErrorCode::Internal, format!("读取预览失败：{e}")))
}

/// 固定修订后在阻塞池组装有界详情；一次只允许一个查询，避免复制大文字属性时无限并发。
#[tauri::command]
pub(crate) async fn read_layers(
    request: LayerRequest,
    host: tauri::State<'_, WorkspaceHost>,
) -> Result<LayerResponse, WorkspaceError> {
    check_version(request.protocol_version)?;
    let guard = SingleOperation::acquire(&host.inspecting)?;
    let lease = host.inspection(request.clone()).await?;
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = guard;
        let result = match request.query {
            LayerQuery::List { offset, limit } => lease
                .layers(offset, limit)
                .map(|page| LayerResult::List { page }),
            LayerQuery::Details {
                layer_id,
                text_start,
            } => lease
                .layer_details(layerlens_core::psd::LayerId(layer_id), text_start)
                .map(|details| LayerResult::Details {
                    details: Box::new(details),
                }),
        }
        .map_err(|e| crate::workspace::domain_error(&e))?;
        Ok(LayerResponse {
            document_id: lease.document_id().get().to_string(),
            revision: lease.revision_id().get().to_string(),
            result,
        })
    })
    .await
    .map_err(|e| error(WorkspaceErrorCode::Internal, format!("读取图层失败：{e}")))?
}

#[tauri::command]
pub(crate) async fn choose_psd_files<R: tauri::Runtime>(
    request: AppInfoRequest,
    window: tauri::WebviewWindow<R>,
    host: tauri::State<'_, WorkspaceHost>,
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
