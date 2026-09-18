//! 真实文件读取与后台服务的集成验证；源文件始终由核心只读访问。

#![cfg(windows)]

use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use layerlens_core::documents::{
    DocumentId, DocumentService, DocumentServiceConfig, OpenJob, OpenOutcome, PreviewAccounting,
    PreviewOutcome, ResourceAccounting, RevisionId,
};
use layerlens_core::psd::{ParseLimits, PsdDocument, PsdErrorCode, Support};

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        loop {
            let directory = std::env::temp_dir().join(format!(
                "layerlens-documents-{}-{}",
                std::process::id(),
                TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ));
            match std::fs::create_dir(&directory) {
                Ok(()) => return Self(directory),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("无法建立测试目录：{error}"),
            }
        }
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/psd")
        .join(name)
}

fn opened(job: OpenJob) -> (DocumentId, RevisionId) {
    match job.wait_timeout(Duration::from_secs(10)).unwrap().outcome {
        OpenOutcome::Opened {
            document_id,
            revision_id,
            ..
        } => (document_id, revision_id),
        ref outcome => panic!("预期成功打开，实际 {outcome:?}"),
    }
}

#[test]
fn source_replacement_and_failed_reload_leave_old_revision_intact() {
    let temp = TempDir::new();
    let path = temp.0.join("design.psd");
    std::fs::copy(fixture("bitmap-raw.psd"), &path).unwrap();
    let mut service = DocumentService::new(DocumentServiceConfig::default()).unwrap();
    let (document, revision) = opened(service.open(&path).unwrap());
    let old = service.lease(document).unwrap();
    let hash = old.source_sha256().to_owned();
    let before = service.snapshot().resources;

    std::fs::write(&path, b"invalid PSD").unwrap();
    let failed = service.reload(document).unwrap().wait();
    assert!(matches!(failed.outcome, OpenOutcome::Failed(_)));
    assert_eq!(service.lease(document).unwrap().revision_id(), revision);
    assert_eq!(service.snapshot().resources, before);
    assert_eq!(service.snapshot().active_document, Some(document));

    std::fs::remove_file(&path).unwrap();
    assert_eq!(old.source_sha256(), hash);
    assert_eq!(old.info().width, 4);
    std::fs::copy(fixture("metadata-text.psd"), &path).unwrap();
    let (reloaded_document, new_revision) = opened(service.reload(document).unwrap());
    assert_eq!(reloaded_document, document);
    assert_ne!(new_revision, revision);
    assert_ne!(service.lease(document).unwrap().source_sha256(), hash);
    assert_eq!(old.source_sha256(), hash);
    assert_eq!(service.snapshot().resources.live_revisions, 2);
    service.close(document).unwrap();
    assert_eq!(service.snapshot().resources, before);
    service.shutdown().unwrap();
    assert_eq!(old.info().width, 4);
    drop(old);
    assert_eq!(service.snapshot().resources, ResourceAccounting::default());
}

#[test]
fn same_filename_in_distinct_directories_opens_distinct_documents() {
    let first = TempDir::new();
    let second = TempDir::new();
    let left = first.0.join("design.psd");
    let right = second.0.join("design.psd");
    std::fs::copy(fixture("bitmap-raw.psd"), &left).unwrap();
    std::fs::copy(fixture("bitmap-raw.psd"), &right).unwrap();
    let service = DocumentService::new(DocumentServiceConfig::default()).unwrap();
    let (left_id, _) = opened(service.open(left).unwrap());
    let (right_id, _) = opened(service.open(right).unwrap());
    assert_ne!(left_id, right_id);
    assert_eq!(service.snapshot().documents.len(), 2);
    service.close(left_id).unwrap();
    assert_eq!(service.snapshot().active_document, Some(right_id));
}

#[test]
fn dropping_service_keeps_external_revision_readable() {
    let service = DocumentService::new(DocumentServiceConfig::default()).unwrap();
    let (document, _) = opened(service.open(fixture("text-engine-72.psd")).unwrap());
    let lease = service.lease(document).unwrap();
    let hash = lease.source_sha256().to_owned();
    drop(service);
    assert_eq!(lease.source_sha256(), hash);
    assert_eq!(lease.info().layers.len(), 27);
}

fn decode(bytes: &[u8]) -> (u32, u32, Vec<u8>) {
    let mut reader = png::Decoder::new(bytes).read_info().unwrap();
    let mut pixels = vec![0; reader.output_buffer_size()];
    let frame = reader.next_frame(&mut pixels).unwrap();
    assert_eq!(frame.color_type, png::ColorType::Rgba);
    pixels.truncate(frame.buffer_size());
    (frame.width, frame.height, pixels)
}

#[test]
fn scheduled_png_matches_saved_pixels_and_preserves_capability() {
    let service = DocumentService::new(DocumentServiceConfig::default()).unwrap();
    for name in [
        "bitmap-raw.psd",
        "bitmap-rle.psd",
        "resources-groups-alpha-raw.psd",
        "resources-groups-alpha-rle.psd",
    ] {
        let source = std::fs::read(fixture(name)).unwrap();
        let reference = PsdDocument::from_bytes(&source, ParseLimits::default()).unwrap();
        let expected = decode(&reference.preview_png().unwrap());
        let (document, revision) = opened(service.open(fixture(name)).unwrap());
        let result = service
            .preview(document)
            .unwrap()
            .wait_timeout(Duration::from_secs(10))
            .unwrap();
        let PreviewOutcome::Ready(image) = &result.outcome else {
            panic!("预览失败：{result:?}")
        };
        let actual = decode(image.png());
        assert_eq!(actual, expected);
        assert_eq!((image.width(), image.height()), (actual.0, actual.1));
        assert_eq!(image.revision_id(), revision);
        assert_eq!(image.capability(), &reference.info().preview);
        if name.starts_with("resources-") {
            // 独立 fixture 预期，不仅比较两个共用解析器的入口。
            assert_eq!(
                actual.2,
                [
                    32, 64, 96, 255, 30, 60, 90, 85, 24, 54, 84, 170, 255, 255, 255, 0
                ]
            );
            assert_eq!(image.capability().status, Support::Partial);
        }
        service.close(document).unwrap();
        assert_eq!(service.snapshot().resources, ResourceAccounting::default());
        assert_eq!(std::fs::read(fixture(name)).unwrap(), source);
    }
}

#[test]
fn preview_uses_fixed_source_and_png_survives_source_and_service_release() {
    let temp = TempDir::new();
    let path = temp.0.join("design.psd");
    std::fs::copy(fixture("bitmap-raw.psd"), &path).unwrap();
    let mut service = DocumentService::new(DocumentServiceConfig::default()).unwrap();
    let (document, _) = opened(service.open(&path).unwrap());
    std::fs::remove_file(&path).unwrap();
    let result = service
        .preview(document)
        .unwrap()
        .wait_timeout(Duration::from_secs(10))
        .unwrap();
    let PreviewOutcome::Ready(image) = &result.outcome else {
        panic!("预览失败：{result:?}")
    };
    service.close(document).unwrap();
    service.shutdown().unwrap();
    assert_eq!(service.snapshot().resources, ResourceAccounting::default());
    assert_eq!(
        service.snapshot().previews.resources.output_bytes,
        image.charged_bytes()
    );
    assert_eq!(decode(image.png()).0, 4);
    drop(result);
    assert_eq!(
        service.snapshot().previews.resources,
        PreviewAccounting::default()
    );
}

#[test]
fn missing_composite_returns_identifiable_failure_and_keeps_metadata() {
    let service = DocumentService::new(DocumentServiceConfig::default()).unwrap();
    let (document, _) = opened(service.open(fixture("bitmap-no-composite.psd")).unwrap());
    let result = service
        .preview(document)
        .unwrap()
        .wait_timeout(Duration::from_secs(10))
        .unwrap();
    assert!(
        matches!(&result.outcome, PreviewOutcome::Failed(error) if matches!(error.as_ref(),
        layerlens_core::documents::DocumentError::Preview { source, .. } if source.code == PsdErrorCode::PreviewUnavailable))
    );
    assert_eq!(service.lease(document).unwrap().info().width, 4);
    assert_eq!(
        service.snapshot().previews.resources,
        PreviewAccounting::default()
    );
}
