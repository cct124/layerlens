//! 将核心状态映射为有序快照，只保留当前画布的额外 PNG 引用。
//! 切换丢弃预览句柄而不取消共享计算；核心缓存仍按自己的 LRU 与预算管理。

use std::{collections::BTreeMap, path::Path};

use layerlens_core::{
    IPC_PROTOCOL_VERSION,
    documents::{
        DocumentId, DocumentLease, DocumentService, DocumentServiceConfig, JobStatus, OpenJob,
        OpenJobId, OpenOutcome, PreviewJob, PreviewLease, PreviewOutcome, RevisionId,
    },
    workspace_contract::*,
};

use super::{domain_error, error};

struct ActivePreview {
    document: DocumentId,
    revision: RevisionId,
    job: Option<PreviewJob>,
    image: Option<PreviewLease>,
    state: WorkspacePreviewState,
}

pub(super) struct Engine {
    pub(super) service: DocumentService,
    opens: BTreeMap<OpenJobId, (String, OpenJob)>,
    preview: Option<ActivePreview>,
    notices: Vec<WorkspaceNotice>,
    notice_sequence: u64,
    sequence: u64,
    last: Option<WorkspaceSnapshot>,
}

impl Engine {
    pub(super) fn new(config: DocumentServiceConfig) -> Result<Self, WorkspaceError> {
        Ok(Self {
            service: DocumentService::new(config).map_err(|e| domain_error(&e))?,
            opens: BTreeMap::new(),
            preview: None,
            notices: Vec::new(),
            notice_sequence: 0,
            sequence: 0,
            last: None,
        })
    }

    fn document(&self, id: &str) -> Result<DocumentId, WorkspaceError> {
        self.service
            .snapshot()
            .documents
            .iter()
            .find(|d| d.id.get().to_string() == id)
            .map(|d| d.id)
            .ok_or_else(|| error(WorkspaceErrorCode::NotFound, "文档不存在或已经关闭。"))
    }

    pub(super) fn act(&mut self, action: WorkspaceAction) -> Result<(), WorkspaceError> {
        match action {
            WorkspaceAction::Snapshot {} => {}
            WorkspaceAction::Open { path } => {
                if path.is_empty()
                    || path.len() > 32768
                    || path.contains('\0')
                    || !Path::new(&path).is_absolute()
                {
                    return Err(error(
                        WorkspaceErrorCode::InvalidInput,
                        "请选择有效的本机 PSD 文件绝对路径。",
                    ));
                }
                let job = self.service.open(&path).map_err(|e| domain_error(&e))?;
                let name = Path::new(&path)
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy();
                self.opens.insert(job.id(), (format!("打开 {name}"), job));
            }
            WorkspaceAction::Activate { document_id } => {
                self.service
                    .activate(self.document(&document_id)?)
                    .map_err(|e| domain_error(&e))?;
            }
            WorkspaceAction::Close { document_id } => {
                self.service
                    .close(self.document(&document_id)?)
                    .map_err(|e| domain_error(&e))?;
            }
            WorkspaceAction::Reload { document_id } => {
                let id = self.document(&document_id)?;
                let name = self
                    .service
                    .snapshot()
                    .documents
                    .iter()
                    .find(|d| d.id == id)
                    .and_then(|d| d.path.file_name())
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                let job = self.service.reload(id).map_err(|e| domain_error(&e))?;
                self.opens.insert(job.id(), (format!("重载 {name}"), job));
            }
            WorkspaceAction::SelectLayer {
                document_id,
                revision,
                layer_id,
            } => {
                let lease = self.inspection_lease(&document_id, &revision)?;
                self.service
                    .select_layer(&lease, layer_id.map(layerlens_core::psd::LayerId))
                    .map_err(|e| domain_error(&e))?;
            }
            WorkspaceAction::RetryPreview {} => {
                // 重试只释放本适配层引用，不隐式取消另一个调用方合并的工作。
                self.preview = None;
            }
            WorkspaceAction::Cancel { job_id } => {
                if let Some((id, _)) = self
                    .opens
                    .iter()
                    .find(|(id, _)| format!("open-{}", id.get()) == job_id)
                {
                    self.service.cancel(*id);
                } else if let Some(job) = self
                    .preview
                    .as_ref()
                    .and_then(|p| p.job.as_ref())
                    .filter(|j| format!("preview-{}", j.id().get()) == job_id)
                {
                    self.service.cancel_preview(job.id());
                } else {
                    return Err(error(
                        WorkspaceErrorCode::NotFound,
                        "作业已经结束或不存在。",
                    ));
                }
            }
            WorkspaceAction::DismissNotice { notice_id } => {
                self.notices.retain(|n| n.id != notice_id)
            }
        }
        Ok(())
    }

    fn notice(&mut self, message: String) {
        self.notice_sequence += 1;
        if self.notices.len() == 16 {
            self.notices.remove(0);
        }
        self.notices.push(WorkspaceNotice {
            id: self.notice_sequence.to_string(),
            message,
        });
    }

    pub(super) fn refresh(&mut self) -> Result<(WorkspaceSnapshot, bool), WorkspaceError> {
        let finished: Vec<_> = self
            .opens
            .iter()
            .filter_map(|(id, (label, job))| {
                if let JobStatus::Finished(done) = job.status() {
                    Some((*id, label.clone(), done))
                } else {
                    None
                }
            })
            .collect();
        for (id, label, done) in finished {
            match &done.outcome {
                OpenOutcome::Failed(e) => self.notice(format!("{label}：{e}")),
                OpenOutcome::Cancelled => self.notice(format!("{label}：已取消")),
                OpenOutcome::Opened { .. } => {}
            }
            self.opens.remove(&id);
        }

        let core = self.service.snapshot();
        let active = core
            .active_document
            .and_then(|id| core.documents.iter().find(|d| d.id == id));
        let target = active.map(|d| (d.id, d.revision));
        if self.preview.as_ref().map(|p| (p.document, p.revision)) != target {
            self.preview = target.map(|(document, revision)| ActivePreview {
                document,
                revision,
                job: None,
                image: None,
                state: WorkspacePreviewState::Pending {
                    job_id: None,
                    status: WorkspaceJobPhase::Queued,
                },
            });
        }
        if let Some(p) = &mut self.preview {
            if p.job.is_none() && matches!(p.state, WorkspacePreviewState::Pending { .. }) {
                match self.service.preview(p.document) {
                    Ok(job) => p.job = Some(job),
                    // 共用 FIFO 满载时等待名额，始终只有一个活动预览请求。
                    Err(DocumentError::QueueFull { .. }) => {}
                    Err(e) => {
                        p.state = WorkspacePreviewState::Failed {
                            error: domain_error(&e),
                        }
                    }
                }
            }
            if let Some(job) = &p.job {
                match job.status() {
                    JobStatus::Finished(done) => {
                        p.state = match &done.outcome {
                            PreviewOutcome::Ready(image) if image.revision_id() == p.revision => {
                                p.image = Some(image.clone());
                                WorkspacePreviewState::Ready {
                                    cache_hit: done.cache_hit,
                                }
                            }
                            // snapshot 与 preview 调用之间可能完成重载；新图不能冠以旧修订。
                            PreviewOutcome::Ready(_) => WorkspacePreviewState::Pending {
                                job_id: None,
                                status: WorkspaceJobPhase::Queued,
                            },
                            PreviewOutcome::Cancelled => WorkspacePreviewState::Cancelled,
                            PreviewOutcome::Failed(e) => WorkspacePreviewState::Failed {
                                error: domain_error(e),
                            },
                        };
                        // 终态句柄也持有 PNG，读取完成立即释放。
                        p.job = None;
                    }
                    status => {
                        p.state = WorkspacePreviewState::Pending {
                            job_id: Some(format!("preview-{}", job.id().get())),
                            status: phase(&status),
                        }
                    }
                }
            }
        }
        let mut documents = Vec::with_capacity(core.documents.len());
        for d in &core.documents {
            documents.push(WorkspaceDocument {
                id: d.id.get().to_string(),
                revision: d.revision.get().to_string(),
                name: d
                    .path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                path: d.path.to_string_lossy().into_owned(),
                width: d.width,
                height: d.height,
                layer_count: d.layer_count,
                color_mode: d.color_mode.clone(),
                bit_depth: d.bit_depth,
                preview_note: d.preview_note.clone(),
                selected_layer_id: d.selected_layer.map(|id| id.0),
            });
        }
        let mut snapshot = WorkspaceSnapshot {
            selection: super::selection::summary(core.selection),
            protocol_version: IPC_PROTOCOL_VERSION,
            sequence: self.sequence.to_string(),
            documents,
            active_document_id: core.active_document.map(|id| id.get().to_string()),
            jobs: self
                .opens
                .iter()
                .filter_map(|(id, (label, job))| {
                    let status = job.status();
                    (!matches!(status, JobStatus::Finished(_))).then(|| WorkspaceJob {
                        id: format!("open-{}", id.get()),
                        label: label.clone(),
                        phase: phase(&status),
                    })
                })
                .collect(),
            preview: self.preview.as_ref().map(|p| WorkspacePreview {
                document_id: p.document.get().to_string(),
                revision: p.revision.get().to_string(),
                state: p.state.clone(),
            }),
            notices: self.notices.clone(),
            resources: WorkspaceResources {
                source_bytes: core.resources.source_bytes.to_string(),
                decoded_bytes: core.previews.resources.decoded_bytes.to_string(),
                output_bytes: core.previews.resources.output_bytes.to_string(),
                cache_bytes: core.previews.cache_bytes.to_string(),
            },
            shutting_down: core.shutting_down,
        };
        let changed = self.last.as_ref() != Some(&snapshot);
        if changed {
            self.sequence = self
                .sequence
                .checked_add(1)
                .ok_or_else(|| error(WorkspaceErrorCode::Internal, "工作区通知序号已耗尽。"))?;
            snapshot.sequence = self.sequence.to_string();
            self.last = Some(snapshot.clone());
        }
        Ok((snapshot, changed))
    }

    pub(super) fn image(
        &self,
        document: &str,
        revision: &str,
    ) -> Result<PreviewLease, WorkspaceError> {
        self.preview
            .as_ref()
            .filter(|p| {
                p.document.get().to_string() == document && p.revision.get().to_string() == revision
            })
            .and_then(|p| p.image.clone())
            .ok_or_else(|| {
                error(
                    WorkspaceErrorCode::StalePreview,
                    "预览已切换或尚未就绪，请读取当前工作区。",
                )
            })
    }

    pub(super) fn inspection_lease(
        &self,
        document: &str,
        revision: &str,
    ) -> Result<DocumentLease, WorkspaceError> {
        let lease = self
            .service
            .lease(self.document(document)?)
            .map_err(|e| domain_error(&e))?;
        if lease.revision_id().get().to_string() != revision {
            return Err(error(
                WorkspaceErrorCode::StaleRevision,
                "文档修订已变化，请重新读取图层。",
            ));
        }
        Ok(lease)
    }

    pub(super) fn shutdown(&mut self) -> Result<(), WorkspaceError> {
        self.preview = None;
        self.opens.clear();
        self.service.shutdown().map_err(|e| domain_error(&e))
    }
}

use layerlens_core::documents::DocumentError;

fn phase<C>(status: &JobStatus<C>) -> WorkspaceJobPhase {
    match status {
        JobStatus::Queued => WorkspaceJobPhase::Queued,
        JobStatus::Running => WorkspaceJobPhase::Running,
        JobStatus::Cancelling => WorkspaceJobPhase::Cancelling,
        JobStatus::Finishing | JobStatus::Finished(_) => WorkspaceJobPhase::Finishing,
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
