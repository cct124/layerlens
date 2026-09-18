//! 通过通道控制解析检查点，验证公开生命周期，不依赖休眠或磁盘速度。

use std::{
    error::Error,
    path::{Path, PathBuf},
    sync::{Arc, mpsc},
    time::Duration,
};

use crate::psd::{ParseLimits, PsdDocument, PsdError, PsdErrorCode};

use super::*;

const DEADLINE: Duration = Duration::from_secs(10);

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/psd")
        .join(name)
}

fn parse(path: &Path, limits: ParseLimits) -> Result<PsdDocument, PsdError> {
    PsdDocument::from_bytes(&std::fs::read(path).unwrap(), limits)
}

fn service(config: DocumentServiceConfig) -> DocumentService {
    DocumentService::with_loader(config, Box::new(parse)).unwrap()
}

fn done(job: &OpenJob) -> Arc<JobCompletion> {
    job.wait_timeout(DEADLINE).expect("作业应在测试期限内完成")
}

fn opened(job: &OpenJob) -> (DocumentId, RevisionId, bool) {
    match &done(job).outcome {
        OpenOutcome::Opened {
            document_id,
            revision_id,
            reused,
        } => (*document_id, *revision_id, *reused),
        outcome => panic!("预期成功打开，实际 {outcome:?}"),
    }
}

fn open(service: &DocumentService, name: &str) -> DocumentId {
    opened(&service.open(fixture(name)).unwrap()).0
}

fn assert_cancelled(job: &OpenJob) {
    assert!(matches!(done(job).outcome, OpenOutcome::Cancelled));
}

/// 未显式继续时在析构中解除阻塞，断言失败不会让服务 Drop 永久等待。
struct Entered {
    path: PathBuf,
    resume: Option<mpsc::Sender<bool>>,
}

impl Entered {
    fn release(mut self) {
        self.resume.take().unwrap().send(false).unwrap();
    }
    fn panic(mut self) {
        self.resume.take().unwrap().send(true).unwrap();
    }
}

impl Drop for Entered {
    fn drop(&mut self) {
        if let Some(resume) = self.resume.take() {
            let _ = resume.send(false);
        }
    }
}

fn controlled(config: DocumentServiceConfig) -> (DocumentService, mpsc::Receiver<Entered>) {
    let (entered, calls) = mpsc::channel();
    let service = DocumentService::with_loader(
        config,
        Box::new(move |path, limits| {
            // 先取得解析结果，再阻塞返回；可精确验证成功数据已就绪时取消仍阻止发布。
            let result = parse(path, limits);
            let (resume, resumed) = mpsc::channel();
            // 接收端析构会释放所有排队检查点；发送失败同样通过 Entered::drop 释放。
            let _ = entered.send(Entered {
                path: path.to_owned(),
                resume: Some(resume),
            });
            if resumed.recv().unwrap() {
                panic!("模拟第三方解析 panic");
            }
            result
        }),
    )
    .unwrap();
    (service, calls)
}

fn entered(calls: &mpsc::Receiver<Entered>) -> Entered {
    calls.recv_timeout(DEADLINE).unwrap()
}

#[test]
fn same_path_reuses_pending_and_published_documents() {
    let (service, calls) = controlled(DocumentServiceConfig::default());
    let path = fixture("bitmap-raw.psd");
    let first = service.open(&path).unwrap();
    let call = entered(&calls);
    assert_eq!(call.path, std::fs::canonicalize(&path).unwrap());
    let duplicate = service.open(&path).unwrap();
    assert_eq!(first.id(), duplicate.id());
    assert_eq!(service.snapshot().pending_jobs.len(), 1);
    call.release();
    let (document, revision, reused) = opened(&first);
    assert!(!reused);
    assert_eq!(
        opened(&service.open(path).unwrap()),
        (document, revision, true)
    );
    let alias = fixture("../psd/bitmap-raw.psd");
    assert_eq!(
        opened(&service.open(alias).unwrap()),
        (document, revision, true)
    );
    assert_eq!(service.snapshot().documents.len(), 1);
    assert!(calls.try_recv().is_err());
}

#[cfg(windows)]
#[test]
fn windows_case_alias_uses_canonical_path_identity() {
    let service = service(DocumentServiceConfig::default());
    let first = open(&service, "bitmap-raw.psd");
    let path = fixture("BITMAP-RAW.PSD");
    assert_eq!(opened(&service.open(path).unwrap()).0, first);
    assert_eq!(service.snapshot().documents.len(), 1);
}

#[test]
fn failures_keep_active_document_and_preserve_error_causes() {
    let service = service(DocumentServiceConfig::default());
    let document = open(&service, "bitmap-raw.psd");
    let before = service.snapshot();
    let invalid = service.open(fixture("rgb16.psd")).unwrap();
    let completion = done(&invalid);
    let OpenOutcome::Failed(error) = &completion.outcome else {
        panic!("应返回解析错误");
    };
    assert!(
        matches!(error.as_ref(), DocumentError::Parse { source, .. } if source.code == PsdErrorCode::Unsupported)
    );
    assert!(error.source().is_some());
    let missing = service.open(fixture("does-not-exist.psd")).unwrap();
    assert!(
        matches!(&done(&missing).outcome, OpenOutcome::Failed(error) if matches!(error.as_ref(), DocumentError::Path { .. }))
    );
    let after = service.snapshot();
    assert_eq!(after.active_document, Some(document));
    assert_eq!(after.documents, before.documents);
    assert_eq!(after.resources, before.resources);
}

#[test]
fn newer_open_and_manual_activation_take_precedence_over_old_completion() {
    let (service, calls) = controlled(DocumentServiceConfig::default());
    let initial = service.open(fixture("bitmap-raw.psd")).unwrap();
    entered(&calls).release();
    let original = opened(&initial).0;
    let older = service.open(fixture("bitmap-rle.psd")).unwrap();
    let old_call = entered(&calls);
    let newer = service.open(fixture("metadata-text.psd")).unwrap();
    old_call.release();
    let new_call = entered(&calls);
    opened(&older);
    assert_eq!(service.snapshot().active_document, Some(original));
    new_call.release();
    let newest = opened(&newer).0;
    assert_eq!(service.snapshot().active_document, Some(newest));
    let late = service.open(fixture("text-engine-72.psd")).unwrap();
    let late_call = entered(&calls);
    service.activate(original).unwrap();
    late_call.release();
    opened(&late);
    assert_eq!(service.snapshot().active_document, Some(original));
    assert_eq!(service.snapshot().documents.len(), 4);
}

#[test]
fn close_chooses_right_then_left_and_last_close_keeps_service_usable() {
    let service = service(DocumentServiceConfig::default());
    let first = open(&service, "bitmap-raw.psd");
    let middle = open(&service, "bitmap-rle.psd");
    let last = open(&service, "metadata-text.psd");
    service.activate(middle).unwrap();
    service.close(middle).unwrap();
    assert_eq!(service.snapshot().active_document, Some(last));
    service.close(last).unwrap();
    assert_eq!(service.snapshot().active_document, Some(first));
    service.close(first).unwrap();
    assert_eq!(service.snapshot().active_document, None);
    assert_eq!(service.snapshot().resources, ResourceAccounting::default());
    assert!(matches!(
        service.lease(first),
        Err(DocumentError::NotFound(_))
    ));
    assert!(matches!(
        service.close(first),
        Err(DocumentError::NotFound(_))
    ));
    assert!(matches!(
        service.activate(first),
        Err(DocumentError::NotFound(_))
    ));
    let reopened = open(&service, "bitmap-raw.psd");
    assert_ne!(first, reopened);
}

#[test]
fn reload_replaces_revision_without_mutating_leases_or_stealing_focus() {
    let service = service(DocumentServiceConfig::default());
    let first = open(&service, "bitmap-raw.psd");
    let retained = service.lease(first).unwrap();
    let other = open(&service, "bitmap-rle.psd");
    let reloaded = service.reload(first).unwrap();
    let (document, revision, reused) = opened(&reloaded);
    assert_eq!(document, first);
    assert!(!reused);
    assert_ne!(revision, retained.revision_id());
    assert_eq!(service.lease(first).unwrap().revision_id(), revision);
    assert_eq!(service.snapshot().active_document, Some(other));
    assert_eq!(service.snapshot().resources.live_revisions, 3);
    service.close(first).unwrap();
    assert_eq!(retained.info().width, 4);
    assert_eq!(service.snapshot().resources.live_revisions, 2);
    drop(retained);
    assert_eq!(service.snapshot().resources.live_revisions, 1);
}

#[test]
fn queued_cancel_never_loads_and_running_cancel_keeps_reservation_until_drained() {
    let config = DocumentServiceConfig {
        max_pending_jobs: 2,
        ..Default::default()
    };
    let (service, calls) = controlled(config);
    let running = service.open(fixture("bitmap-raw.psd")).unwrap();
    let call = entered(&calls);
    let queued = service.open(fixture("bitmap-rle.psd")).unwrap();
    assert!(matches!(
        service.open(fixture("metadata-text.psd")),
        Err(DocumentError::QueueFull { limit: 2 })
    ));
    assert!(service.cancel(queued.id()));
    assert_cancelled(&queued);
    assert_eq!(done(&queued).executed_for, Duration::ZERO);
    assert!(!service.cancel(queued.id()));
    assert!(service.cancel(running.id()));
    assert!(matches!(running.status(), JobStatus::Cancelling));
    let snapshot = service.snapshot();
    assert_eq!(snapshot.pending_jobs.len(), 1);
    assert_eq!(
        snapshot.resources,
        ResourceAccounting {
            live_revisions: 1,
            source_bytes: config.parse_limits.max_file_bytes,
            declared_pixels: config.parse_limits.max_total_pixels,
        }
    );
    assert!(running.wait_timeout(Duration::ZERO).is_none());
    call.release();
    assert_cancelled(&running);
    assert_eq!(service.snapshot().resources, ResourceAccounting::default());
    assert!(service.snapshot().documents.is_empty());
    assert!(calls.try_recv().is_err());
    let recovery = service.open(fixture("bitmap-rle.psd")).unwrap();
    entered(&calls).release();
    opened(&recovery);
    assert!(!service.cancel(recovery.id()));
    assert!(matches!(
        done(&recovery).outcome,
        OpenOutcome::Opened { .. }
    ));
}

#[test]
fn cancelling_running_job_does_not_free_queue_slot_early() {
    let (service, calls) = controlled(DocumentServiceConfig {
        max_pending_jobs: 1,
        ..Default::default()
    });
    let running = service.open(fixture("bitmap-raw.psd")).unwrap();
    let call = entered(&calls);
    assert!(service.cancel(running.id()));
    assert!(matches!(
        service.open(fixture("bitmap-rle.psd")),
        Err(DocumentError::QueueFull { .. })
    ));
    call.release();
    assert_cancelled(&running);
}

#[test]
fn superseded_reloads_only_publish_latest_and_close_prevents_resurrection() {
    let (service, calls) = controlled(DocumentServiceConfig {
        max_pending_jobs: 2,
        ..Default::default()
    });
    let initial = service.open(fixture("bitmap-raw.psd")).unwrap();
    entered(&calls).release();
    let (document, revision, _) = opened(&initial);
    let older = service.reload(document).unwrap();
    let old_call = entered(&calls);
    let middle = service.reload(document).unwrap();
    let latest = service.reload(document).unwrap();
    assert_cancelled(&middle);
    assert_eq!(service.snapshot().pending_jobs.len(), 2);
    assert_eq!(service.lease(document).unwrap().revision_id(), revision);
    old_call.release();
    let latest_call = entered(&calls);
    assert_cancelled(&older);
    assert_eq!(service.lease(document).unwrap().revision_id(), revision);
    latest_call.release();
    let (_, new_revision, _) = opened(&latest);
    assert_ne!(revision, new_revision);
    let closing = service.reload(document).unwrap();
    let closing_call = entered(&calls);
    service.close(document).unwrap();
    assert!(service.snapshot().documents.is_empty());
    assert_eq!(service.snapshot().resources.live_revisions, 1);
    closing_call.release();
    assert_cancelled(&closing);
    assert_eq!(service.snapshot().resources, ResourceAccounting::default());
    assert!(service.snapshot().documents.is_empty());
}

#[test]
fn rejected_reload_does_not_cancel_previous_request() {
    let (service, calls) = controlled(DocumentServiceConfig {
        max_pending_jobs: 1,
        ..Default::default()
    });
    let initial = service.open(fixture("bitmap-raw.psd")).unwrap();
    entered(&calls).release();
    let document = opened(&initial).0;
    let reload = service.reload(document).unwrap();
    let call = entered(&calls);
    assert!(matches!(
        service.reload(document),
        Err(DocumentError::QueueFull { .. })
    ));
    assert!(matches!(reload.status(), JobStatus::Running));
    call.release();
    opened(&reload);
}

#[test]
fn unresolved_alias_queued_before_close_cannot_reopen_closed_document() {
    let (service, calls) = controlled(DocumentServiceConfig::default());
    let initial = service.open(fixture("bitmap-raw.psd")).unwrap();
    entered(&calls).release();
    let document = opened(&initial).0;
    let blocking = service.open(fixture("bitmap-rle.psd")).unwrap();
    let call = entered(&calls);
    // 在 Windows absolute 会折叠 ..，保留逐个组件的大小写别名直到后台 canonicalize。
    let alias = if cfg!(windows) {
        fixture("BITMAP-RAW.PSD")
    } else {
        fixture("../psd/bitmap-raw.psd")
    };
    let alias_job = service.open(alias).unwrap();
    service.close(document).unwrap();
    call.release();
    opened(&blocking);
    assert_cancelled(&alias_job);
    assert_eq!(service.snapshot().documents.len(), 1);
    let reopened = service.open(fixture("bitmap-raw.psd")).unwrap();
    entered(&calls).release();
    assert_ne!(opened(&reopened).0, document);
}

#[test]
fn closed_lease_stays_charged_and_resource_rejection_is_recoverable() {
    let service = service(DocumentServiceConfig {
        max_live_revisions: 1,
        ..Default::default()
    });
    let document = open(&service, "bitmap-raw.psd");
    let lease = service.lease(document).unwrap();
    let before = service.snapshot().resources;
    service.close(document).unwrap();
    assert_eq!(service.snapshot().resources, before);
    let rejected = service.open(fixture("bitmap-rle.psd")).unwrap();
    assert!(
        matches!(&done(&rejected).outcome, OpenOutcome::Failed(error)
        if matches!(error.as_ref(), DocumentError::ResourceLimit { resource: "修订数量", limit: 1 }))
    );
    assert_eq!(service.snapshot().resources, before);
    drop(lease);
    assert_eq!(service.snapshot().resources, ResourceAccounting::default());
    open(&service, "bitmap-rle.psd");
}

#[test]
fn source_and_pixel_budgets_reserve_before_parsing_and_settle_after_success() {
    for source_limited in [true, false] {
        let mut config = DocumentServiceConfig::default();
        let resource = if source_limited {
            config.max_source_bytes = config.parse_limits.max_file_bytes;
            "源字节"
        } else {
            config.max_declared_pixels = config.parse_limits.max_total_pixels;
            "声明像素"
        };
        let service = service(config);
        let document = open(&service, "bitmap-raw.psd");
        let lease = service.lease(document).unwrap();
        let usage = service.snapshot().resources;
        assert_eq!(usage.source_bytes, lease.info().resources.source_bytes);
        assert_eq!(
            usage.declared_pixels,
            lease.info().resources.total_declared_pixels
        );
        assert!(usage.source_bytes < config.parse_limits.max_file_bytes);
        let rejected = service.open(fixture("bitmap-rle.psd")).unwrap();
        assert!(
            matches!(&done(&rejected).outcome, OpenOutcome::Failed(error)
            if matches!(error.as_ref(), DocumentError::ResourceLimit { resource: actual, .. } if *actual == resource))
        );
        assert_eq!(service.snapshot().resources, usage);
        drop(lease);
        service.close(document).unwrap();
        open(&service, "bitmap-rle.psd");
    }
}

#[test]
fn parse_failure_and_panic_release_reservation_and_worker_can_continue() {
    let (service, calls) = controlled(DocumentServiceConfig {
        max_live_revisions: 1,
        ..Default::default()
    });
    let failure = service.open(fixture("rgb16.psd")).unwrap();
    entered(&calls).release();
    assert!(matches!(done(&failure).outcome, OpenOutcome::Failed(_)));
    assert_eq!(service.snapshot().resources, ResourceAccounting::default());
    let panic_job = service.open(fixture("bitmap-raw.psd")).unwrap();
    entered(&calls).panic();
    assert!(
        matches!(&done(&panic_job).outcome, OpenOutcome::Failed(error) if matches!(error.as_ref(), DocumentError::WorkerPanicked))
    );
    assert_eq!(service.snapshot().resources, ResourceAccounting::default());
    let recovery = service.open(fixture("bitmap-rle.psd")).unwrap();
    entered(&calls).release();
    opened(&recovery);
}

#[test]
fn shutdown_cancels_pending_rejects_admission_and_preserves_external_leases() {
    let (mut service, calls) = controlled(DocumentServiceConfig::default());
    let initial = service.open(fixture("bitmap-raw.psd")).unwrap();
    entered(&calls).release();
    let document = opened(&initial).0;
    let lease = service.lease(document).unwrap();
    let running = service.reload(document).unwrap();
    let call = entered(&calls);
    let queued = service.open(fixture("bitmap-rle.psd")).unwrap();
    service.request_shutdown();
    assert!(matches!(
        service.open(fixture("metadata-text.psd")),
        Err(DocumentError::ShuttingDown)
    ));
    assert!(matches!(
        service.reload(document),
        Err(DocumentError::ShuttingDown)
    ));
    assert_cancelled(&queued);
    assert!(matches!(running.status(), JobStatus::Cancelling));
    assert_eq!(service.snapshot().resources.live_revisions, 2);
    call.release();
    service.shutdown().unwrap();
    assert_cancelled(&running);
    assert!(service.snapshot().shutting_down);
    assert!(service.snapshot().documents.is_empty());
    assert!(service.snapshot().pending_jobs.is_empty());
    assert_eq!(service.snapshot().resources.live_revisions, 1);
    assert_eq!(lease.info().width, 4);
    drop(lease);
    assert_eq!(service.snapshot().resources, ResourceAccounting::default());
    service.shutdown().unwrap();
}

#[test]
fn invalid_configuration_is_rejected_before_worker_creation() {
    let config = DocumentServiceConfig {
        max_pending_jobs: 0,
        ..Default::default()
    };
    assert!(matches!(
        DocumentService::new(config),
        Err(DocumentError::InvalidConfiguration(_))
    ));
    let config = DocumentServiceConfig {
        max_source_bytes: 1,
        ..Default::default()
    };
    assert!(matches!(
        DocumentService::new(config),
        Err(DocumentError::InvalidConfiguration(_))
    ));
}
