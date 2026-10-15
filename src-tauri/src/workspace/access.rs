//! 桌面 IPC 与开发适配共用的读取边界；保留单次复制准入、后台计算及发布复检。

use super::{WorkspaceHost, check_version, domain_error, error, validate_publication};
use layerlens_core::{documents::PreviewLease, psd::LayerId, workspace_contract::*};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

pub(crate) struct SingleOperation(Arc<AtomicBool>);
impl SingleOperation {
    pub(crate) fn acquire(flag: &Arc<AtomicBool>) -> Result<Self, WorkspaceError> {
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

impl WorkspaceHost {
    /// 两种传输共用一次图像复制额度；编码和发布复检均在阻塞池中完成。
    pub(crate) async fn read_image<T: Send + 'static>(
        &self,
        request: PreviewRequest,
        encode: impl FnOnce(&PreviewLease) -> Result<T, WorkspaceError> + Send + 'static,
    ) -> Result<T, WorkspaceError> {
        check_version(request.protocol_version)?;
        let guard = SingleOperation::acquire(&self.image_reading)?;
        let cleanup = self.cleanup_handle.clone();
        let image = self.image(request).await?;
        #[cfg(feature = "dev-mcp")]
        self.operations
            .testing
            .hit(super::testing::Checkpoint::PreviewPublication)
            .await?;
        tauri::async_runtime::spawn_blocking(move || {
            let _guard = guard;
            let result = encode(&image)?;
            validate_publication(&cleanup, image.document_id(), image.revision_id())?;
            Ok(result)
        })
        .await
        .map_err(|e| error(WorkspaceErrorCode::Internal, format!("读取预览失败：{e}")))?
    }

    /// 固定修订，在既有单查询额度内组装有界图层／文字，清理后拒绝发布。
    pub(crate) async fn read_layers(
        &self,
        request: LayerRequest,
    ) -> Result<LayerResponse, WorkspaceError> {
        check_version(request.protocol_version)?;
        let guard = SingleOperation::acquire(&self.inspecting)?;
        let cleanup = self.cleanup_handle.clone();
        let lease = self.inspection(request.clone()).await?;
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
                    .layer_details(LayerId(layer_id), text_start)
                    .map(|details| LayerResult::Details {
                        details: Box::new(details),
                    }),
            }
            .map_err(|e| domain_error(&e))?;
            validate_publication(&cleanup, lease.document_id(), lease.revision_id())?;
            Ok(LayerResponse {
                document_id: lease.document_id().get().to_string(),
                revision: lease.revision_id().get().to_string(),
                result,
            })
        })
        .await
        .map_err(|e| error(WorkspaceErrorCode::Internal, format!("读取图层失败：{e}")))?
    }
}
