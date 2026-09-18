//! 预览行为测试使用通道检查点，不依赖休眠或真实文件大小。

use super::super::{DocumentServiceConfig, OpenOutcome, ResourceAccounting};
use super::*;
use crate::psd::{ParseLimits, PsdDocument, PsdError, PsdErrorCode};
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Duration,
};

const DEADLINE: Duration = Duration::from_secs(10);

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/psd")
        .join(name)
}
fn parse(path: &Path, limits: ParseLimits) -> Result<PsdDocument, PsdError> {
    PsdDocument::from_bytes(&std::fs::read(path).unwrap(), limits)
}
fn config() -> DocumentServiceConfig {
    DocumentServiceConfig {
        preview: PreviewConfig {
            max_png_bytes: 4096,
            max_output_bytes: 16384,
            max_cache_bytes: 8192,
            max_cache_entries: 2,
            ..PreviewConfig::default()
        },
        ..DocumentServiceConfig::default()
    }
}
fn open(service: &DocumentService, name: &str) -> DocumentId {
    match service
        .open(fixture(name))
        .unwrap()
        .wait_timeout(DEADLINE)
        .unwrap()
        .outcome
    {
        OpenOutcome::Opened { document_id, .. } => document_id,
        ref outcome => panic!("打开失败：{outcome:?}"),
    }
}
fn done(job: &PreviewJob) -> Arc<PreviewCompletion> {
    job.wait_timeout(DEADLINE).expect("预览应按期完成")
}
fn image(job: &PreviewJob) -> PreviewLease {
    match &done(job).outcome {
        PreviewOutcome::Ready(image) => image.clone(),
        other => panic!("预览失败：{other:?}"),
    }
}
fn service(config: DocumentServiceConfig) -> DocumentService {
    DocumentService::with_workers(
        config,
        Box::new(parse),
        Box::new(PsdDocument::preview_png_bounded),
    )
    .unwrap()
}

struct Entered(Option<mpsc::Sender<bool>>);
impl Entered {
    fn release(mut self) {
        self.0.take().unwrap().send(false).unwrap();
    }
    fn panic(mut self) {
        self.0.take().unwrap().send(true).unwrap();
    }
}
impl Drop for Entered {
    fn drop(&mut self) {
        if let Some(resume) = self.0.take() {
            let _ = resume.send(false);
        }
    }
}
fn controlled(config: DocumentServiceConfig) -> (DocumentService, mpsc::Receiver<Entered>) {
    let (entered, calls) = mpsc::channel();
    let service = DocumentService::with_workers(
        config,
        Box::new(parse),
        Box::new(move |document, limit| {
            let result = document.preview_png_bounded(limit);
            let (resume, resumed) = mpsc::channel();
            let _ = entered.send(Entered(Some(resume)));
            if resumed.recv().unwrap() {
                panic!("模拟预览解码 panic");
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
fn same_revision_coalesces_then_hits_cache_without_decoding() {
    let (mut service, calls) = controlled(config());
    let document = open(&service, "bitmap-raw.psd");
    let first = service.preview(document).unwrap();
    let checkpoint = entered(&calls);
    let joined = service.preview(document).unwrap();
    assert_eq!(joined.id(), first.id());
    let snapshot = service.snapshot();
    assert_eq!(snapshot.pending_previews.len(), 1);
    assert_eq!(
        snapshot.previews.resources.decoded_bytes,
        config().parse_limits.max_decoded_bytes
    );
    assert_eq!(snapshot.previews.resources.output_bytes, 4096);
    checkpoint.release();
    let initial = image(&first);
    assert!(!done(&first).cache_hit);
    assert!(done(&first).png_timings.is_some());
    let cached = service.preview(document).unwrap();
    assert!(done(&cached).cache_hit);
    assert_eq!(done(&cached).executed_for, Duration::ZERO);
    assert!(done(&cached).png_timings.is_none());
    assert_eq!(image(&cached).png().as_ptr(), initial.png().as_ptr());
    assert!(calls.try_recv().is_err());
    assert_eq!(service.snapshot().previews.coalesced_requests, 1);
    assert_eq!(service.snapshot().previews.cache_hits, 1);
    service.shutdown().unwrap();
    assert_eq!(service.snapshot().resources, ResourceAccounting::default());
    assert_eq!(service.snapshot().previews.resources.decoded_bytes, 0);
    assert_eq!(
        service.snapshot().previews.resources.output_bytes,
        initial.charged_bytes()
    );
    drop((first, joined, cached, initial));
    assert_eq!(
        service.snapshot().previews.resources,
        PreviewAccounting::default()
    );
}

#[test]
fn lru_touch_evicts_oldest_and_recomputes_only_missing_entry() {
    let service = service(config());
    let a = open(&service, "bitmap-raw.psd");
    let b = open(&service, "bitmap-rle.psd");
    let c = open(&service, "metadata-text.psd");
    drop(image(&service.preview(a).unwrap()));
    drop(image(&service.preview(b).unwrap()));
    assert!(done(&service.preview(a).unwrap()).cache_hit);
    drop(image(&service.preview(c).unwrap()));
    assert_eq!(service.snapshot().previews.evictions, 1);
    assert!(done(&service.preview(a).unwrap()).cache_hit);
    assert!(!done(&service.preview(b).unwrap()).cache_hit);
    let snapshot = service.snapshot().previews;
    assert_eq!(snapshot.cache_entries, 2);
    assert_eq!(snapshot.cache_bytes, snapshot.resources.output_bytes);
}

#[test]
fn retained_results_remain_charged_after_eviction_and_refuse_new_output() {
    let mut config = config();
    config.preview.max_output_bytes = 4096;
    config.preview.max_cache_bytes = 4096;
    let mut service = service(config);
    let a = open(&service, "bitmap-raw.psd");
    let b = open(&service, "bitmap-rle.psd");
    let pinned = service.preview(a).unwrap();
    let held = image(&pinned);
    let failed = done(&service.preview(b).unwrap());
    assert!(
        matches!(&failed.outcome, PreviewOutcome::Failed(error) if matches!(**error, DocumentError::ResourceLimit { resource: "预览输出字节", .. }))
    );
    assert_eq!(service.snapshot().previews.cache_entries, 0);
    assert_eq!(
        service.snapshot().previews.resources.output_bytes,
        held.charged_bytes()
    );
    drop((pinned, held));
    drop(image(&service.preview(b).unwrap()));
    service.shutdown().unwrap();
    assert_eq!(
        service.snapshot().previews.resources,
        PreviewAccounting::default()
    );
}

#[test]
fn output_pressure_releases_unheld_cache_before_decoding() {
    let mut config = config();
    config.preview.max_output_bytes = 4096;
    config.preview.max_cache_bytes = 4096;
    let service = service(config);
    let a = open(&service, "bitmap-raw.psd");
    let b = open(&service, "bitmap-rle.psd");
    drop(image(&service.preview(a).unwrap()));
    drop(image(&service.preview(b).unwrap()));
    let snapshot = service.snapshot().previews;
    assert_eq!(snapshot.evictions, 1);
    assert_eq!(snapshot.cache_entries, 1);
    assert_eq!(snapshot.resources.output_bytes, snapshot.cache_bytes);
}

#[test]
fn cached_requests_do_not_need_free_worker_slots() {
    let mut config = config();
    config.max_pending_jobs = 1;
    let (service, calls) = controlled(config);
    let a = open(&service, "bitmap-raw.psd");
    let b = open(&service, "bitmap-rle.psd");
    let first = service.preview(a).unwrap();
    entered(&calls).release();
    drop(image(&first));
    let busy = service.preview(b).unwrap();
    let checkpoint = entered(&calls);
    assert!(done(&service.preview(a).unwrap()).cache_hit);
    assert!(matches!(
        service.reload(a),
        Err(DocumentError::QueueFull { limit: 1 })
    ));
    checkpoint.release();
    drop(image(&busy));
}

#[test]
fn cache_can_be_disabled_or_smaller_than_a_single_image() {
    for (entries, bytes) in [(0, 8192), (2, 0), (2, 100)] {
        let mut config = config();
        config.preview.max_cache_entries = entries;
        config.preview.max_cache_bytes = bytes;
        let service = service(config);
        let document = open(&service, "bitmap-raw.psd");
        drop(image(&service.preview(document).unwrap()));
        assert_eq!(service.snapshot().previews.cache_entries, 0);
        assert_eq!(
            service.snapshot().previews.resources,
            PreviewAccounting::default()
        );
        assert!(!done(&service.preview(document).unwrap()).cache_hit);
    }
}

#[test]
fn cancel_ready_but_unpublished_result_cancels_all_joined_handles() {
    let (mut service, calls) = controlled(config());
    let document = open(&service, "bitmap-raw.psd");
    let first = service.preview(document).unwrap();
    let checkpoint = entered(&calls);
    let joined = service.preview(document).unwrap();
    assert!(service.cancel_preview(joined.id()));
    assert!(!service.cancel_preview(first.id()));
    assert!(matches!(first.status(), JobStatus::Cancelling));
    assert!(first.wait_timeout(Duration::ZERO).is_none());
    assert_eq!(service.snapshot().pending_previews.len(), 1);
    assert_ne!(service.snapshot().previews.resources.decoded_bytes, 0);
    checkpoint.release();
    assert!(matches!(done(&first).outcome, PreviewOutcome::Cancelled));
    assert!(matches!(done(&joined).outcome, PreviewOutcome::Cancelled));
    assert_eq!(
        service.snapshot().previews.resources,
        PreviewAccounting::default()
    );
    service.shutdown().unwrap();
}

#[test]
fn shared_queue_capacity_and_fifo_preserve_open_preview_order() {
    let mut config = config();
    config.max_pending_jobs = 2;
    let (service, calls) = controlled(config);
    let a = open(&service, "bitmap-raw.psd");
    let b = open(&service, "bitmap-rle.psd");
    let first = service.preview(a).unwrap();
    let checkpoint = entered(&calls);
    let reload = service.reload(b).unwrap();
    assert!(matches!(
        service.preview(b),
        Err(DocumentError::QueueFull { limit: 2 })
    ));
    assert!(matches!(
        service.open(fixture("metadata-text.psd")),
        Err(DocumentError::QueueFull { limit: 2 })
    ));
    checkpoint.release();
    drop(image(&first));
    assert!(matches!(
        reload.wait_timeout(DEADLINE).unwrap().outcome,
        OpenOutcome::Opened { .. }
    ));
    let preview = service.preview(b).unwrap();
    let checkpoint = entered(&calls);
    let reload = service.reload(b).unwrap();
    checkpoint.release();
    let old = image(&preview);
    assert!(matches!(
        reload.wait_timeout(DEADLINE).unwrap().outcome,
        OpenOutcome::Opened { .. }
    ));
    assert_ne!(old.revision_id(), service.lease(b).unwrap().revision_id());
}

#[test]
fn queued_cancellation_never_invokes_renderer_and_releases_slot() {
    let (service, calls) = controlled(config());
    let a = open(&service, "bitmap-raw.psd");
    let b = open(&service, "bitmap-rle.psd");
    let first = service.preview(a).unwrap();
    let checkpoint = entered(&calls);
    let queued = service.preview(b).unwrap();
    assert!(matches!(queued.status(), JobStatus::Queued));
    assert!(service.cancel_preview(queued.id()));
    assert!(matches!(done(&queued).outcome, PreviewOutcome::Cancelled));
    assert_eq!(done(&queued).executed_for, Duration::ZERO);
    assert_eq!(service.snapshot().pending_previews.len(), 1);
    service.close(b).unwrap();
    assert_eq!(service.snapshot().resources.live_revisions, 1);
    checkpoint.release();
    drop(image(&first));
    assert!(calls.try_recv().is_err());
}

#[test]
fn close_cancels_running_and_queued_work_and_frees_source_after_return() {
    let (mut service, calls) = controlled(config());
    let a = open(&service, "bitmap-raw.psd");
    let b = open(&service, "bitmap-rle.psd");
    let first = service.preview(a).unwrap();
    let checkpoint = entered(&calls);
    let queued = service.preview(b).unwrap();
    service.close(b).unwrap();
    assert!(matches!(done(&queued).outcome, PreviewOutcome::Cancelled));
    service.close(a).unwrap();
    assert_eq!(service.snapshot().resources.live_revisions, 1);
    assert!(matches!(
        service.preview(a),
        Err(DocumentError::NotFound(_))
    ));
    checkpoint.release();
    assert!(matches!(done(&first).outcome, PreviewOutcome::Cancelled));
    assert_eq!(service.snapshot().resources, ResourceAccounting::default());
    assert_eq!(
        service.snapshot().previews.resources,
        PreviewAccounting::default()
    );
    service.shutdown().unwrap();
}

#[test]
fn successful_reload_invalidates_queued_old_revision_and_cache() {
    let (service, calls) = controlled(config());
    let a = open(&service, "bitmap-raw.psd");
    let b = open(&service, "bitmap-rle.psd");
    let first = service.preview(a).unwrap();
    let checkpoint = entered(&calls);
    let reload = service.reload(b).unwrap();
    let stale = service.preview(b).unwrap();
    checkpoint.release();
    drop(image(&first));
    assert!(matches!(
        reload.wait_timeout(DEADLINE).unwrap().outcome,
        OpenOutcome::Opened { .. }
    ));
    assert!(matches!(done(&stale).outcome, PreviewOutcome::Cancelled));
    let old_revision = image(&first).revision_id();
    let reload = service.reload(a).unwrap();
    assert!(matches!(
        reload.wait_timeout(DEADLINE).unwrap().outcome,
        OpenOutcome::Opened { .. }
    ));
    assert_eq!(service.snapshot().previews.cache_entries, 0);
    let new = service.preview(a).unwrap();
    entered(&calls).release();
    assert_ne!(image(&new).revision_id(), old_revision);
    assert!(!done(&new).cache_hit);
}

#[test]
fn failed_reload_retains_valid_cached_preview() {
    let fail = Arc::new(AtomicBool::new(false));
    let fail_loader = Arc::clone(&fail);
    let service = DocumentService::with_workers(
        config(),
        Box::new(move |path, limits| {
            if fail_loader.load(Ordering::SeqCst) {
                PsdDocument::from_bytes(b"invalid", limits)
            } else {
                parse(path, limits)
            }
        }),
        Box::new(PsdDocument::preview_png_bounded),
    )
    .unwrap();
    let document = open(&service, "bitmap-raw.psd");
    let old = image(&service.preview(document).unwrap());
    fail.store(true, Ordering::SeqCst);
    assert!(matches!(
        service
            .reload(document)
            .unwrap()
            .wait_timeout(DEADLINE)
            .unwrap()
            .outcome,
        OpenOutcome::Failed(_)
    ));
    let cached = service.preview(document).unwrap();
    assert!(done(&cached).cache_hit);
    assert_eq!(image(&cached).revision_id(), old.revision_id());
}

#[test]
fn shutdown_rejects_requests_and_releases_all_cancelled_work() {
    let (mut service, calls) = controlled(config());
    let a = open(&service, "bitmap-raw.psd");
    let b = open(&service, "bitmap-rle.psd");
    let first = service.preview(a).unwrap();
    let checkpoint = entered(&calls);
    let queued = service.preview(b).unwrap();
    service.request_shutdown();
    assert!(matches!(
        service.preview(a),
        Err(DocumentError::ShuttingDown)
    ));
    assert!(matches!(done(&queued).outcome, PreviewOutcome::Cancelled));
    assert!(matches!(first.status(), JobStatus::Cancelling));
    checkpoint.release();
    service.shutdown().unwrap();
    assert!(matches!(done(&first).outcome, PreviewOutcome::Cancelled));
    assert_eq!(service.snapshot().resources, ResourceAccounting::default());
    assert_eq!(
        service.snapshot().previews.resources,
        PreviewAccounting::default()
    );
    assert!(service.snapshot().pending_previews.is_empty());
}

#[test]
fn renderer_panic_refunds_reservations_and_worker_recovers() {
    let (service, calls) = controlled(config());
    let document = open(&service, "bitmap-raw.psd");
    let failed = service.preview(document).unwrap();
    entered(&calls).panic();
    assert!(
        matches!(&done(&failed).outcome, PreviewOutcome::Failed(error) if matches!(**error, DocumentError::WorkerPanicked))
    );
    assert_eq!(
        service.snapshot().previews.resources,
        PreviewAccounting::default()
    );
    let next = service.preview(document).unwrap();
    entered(&calls).release();
    drop(image(&next));
}

#[test]
fn png_and_decode_limits_fail_without_changing_metadata() {
    for (png_limit, decode_limit) in [(32, 128 * 1024 * 1024), (80, 128 * 1024 * 1024), (4096, 1)] {
        let mut config = config();
        config.preview.max_png_bytes = png_limit;
        config.parse_limits.max_decoded_bytes = decode_limit;
        let service = service(config);
        let document = open(&service, "bitmap-raw.psd");
        let hash = service.lease(document).unwrap().source_sha256().to_owned();
        let failed = done(&service.preview(document).unwrap());
        assert!(
            matches!(&failed.outcome, PreviewOutcome::Failed(error) if matches!(error.as_ref(), DocumentError::Preview { source, .. } if source.code == PsdErrorCode::ResourceLimit))
        );
        assert_eq!(
            service.snapshot().previews.resources,
            PreviewAccounting::default()
        );
        assert_eq!(service.lease(document).unwrap().source_sha256(), hash);
    }
}

#[test]
fn invalid_preview_configuration_is_rejected() {
    let mut config = config();
    config.preview.max_decoded_bytes = 1;
    assert!(matches!(
        DocumentService::new(config),
        Err(DocumentError::InvalidConfiguration(_))
    ));
    config.preview = PreviewConfig {
        max_png_bytes: 0,
        ..PreviewConfig::default()
    };
    assert!(matches!(
        DocumentService::new(config),
        Err(DocumentError::InvalidConfiguration(_))
    ));
    config.preview = PreviewConfig {
        max_output_bytes: 1,
        ..PreviewConfig::default()
    };
    assert!(matches!(
        DocumentService::new(config),
        Err(DocumentError::InvalidConfiguration(_))
    ));
}
