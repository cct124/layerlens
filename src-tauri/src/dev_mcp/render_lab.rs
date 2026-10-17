//! 实验场景入口：显式实例授权、固定修订、有界后台与发布复检。
use super::Instance;
use crate::workspace::{
    SingleOperation, WorkspaceHost, check_size, check_version, domain_error, error,
    validate_publication,
};
use layerlens_core::{
    psd::{RenderProfile, RenderScene},
    workspace_contract::*,
};
use std::sync::Arc;

#[tauri::command]
pub(crate) async fn read_render_scene<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    instance: tauri::State<'_, Arc<Instance>>,
    host: tauri::State<'_, Arc<WorkspaceHost>>,
    request: PreviewRequest,
    profile: RenderProfile,
) -> Result<RenderScene, WorkspaceError> {
    if window.label() != "main"
        || !instance.render_lab
        || (profile.deferred() && !instance.large_render)
    {
        return Err(error(
            WorkspaceErrorCode::InvalidInput,
            "此实例未启用渲染实验",
        ));
    }
    check_version(request.protocol_version)?;
    let _guard = SingleOperation::acquire(&host.image_reading)?;
    let lease = host
        .inspection(LayerRequest {
            protocol_version: request.protocol_version,
            document_id: request.document_id,
            revision: request.revision,
            query: LayerQuery::List {
                offset: 0,
                limit: 64,
            },
        })
        .await?;
    let cleanup = host.cleanup_handle.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = _guard;
        let scene = lease
            .render_scene_with_profile(profile)
            .map_err(|e| domain_error(&e))?;
        check_size(
            &scene,
            if profile.deferred() {
                2 * 1024 * 1024
            } else {
                16 * 1024 * 1024
            },
        )?;
        validate_publication(&cleanup, lease.document_id(), lease.revision_id())?;
        Ok(scene)
    })
    .await
    .map_err(|e| {
        error(
            WorkspaceErrorCode::Internal,
            format!("实验场景读取失败：{e}"),
        )
    })?
}

/// 固定修订逐项读取 PNG；单并发、后台解码、32 MiB 编码上限，清理后禁止发布。
#[tauri::command]
pub(crate) async fn read_render_resource<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    instance: tauri::State<'_, Arc<Instance>>,
    host: tauri::State<'_, Arc<WorkspaceHost>>,
    request: PreviewRequest,
    layer_id: Option<u32>,
    profile: RenderProfile,
) -> Result<tauri::ipc::Response, WorkspaceError> {
    if window.label() != "main"
        || !instance.render_lab
        || !instance.large_render
        || !profile.deferred()
    {
        return Err(error(
            WorkspaceErrorCode::InvalidInput,
            "此实例未启用大稿资源读取",
        ));
    }
    check_version(request.protocol_version)?;
    let guard = SingleOperation::acquire(&host.image_reading)?;
    let lease = host
        .inspection(LayerRequest {
            protocol_version: request.protocol_version,
            document_id: request.document_id,
            revision: request.revision,
            query: LayerQuery::List {
                offset: 0,
                limit: 1,
            },
        })
        .await?;
    let cleanup = host.cleanup_handle.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = guard;
        let bytes = lease
            .render_resource(layer_id.map(layerlens_core::psd::LayerId), profile)
            .map_err(|e| domain_error(&e))?;
        validate_publication(&cleanup, lease.document_id(), lease.revision_id())?;
        Ok(tauri::ipc::Response::new(bytes))
    })
    .await
    .map_err(|e| {
        error(
            WorkspaceErrorCode::Internal,
            format!("实验资源读取失败：{e}"),
        )
    })?
}
