//! 通过真实只读样本验证适配器顺序、修订隔离与引用释放，不使用固定休眠。
use super::*;
use std::time::Duration;

fn fixture(name: &str) -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../crates/layerlens-core/tests/fixtures/psd")
        .join(name)
        .to_string_lossy()
        .into_owned()
}
fn finish_opens(engine: &Engine) {
    for (_, job) in engine.opens.values() {
        assert!(job.wait_timeout(Duration::from_secs(10)).is_some());
    }
}
fn open(engine: &mut Engine, name: &str) -> WorkspaceSnapshot {
    engine
        .act(WorkspaceAction::Open {
            path: fixture(name),
        })
        .unwrap();
    finish_opens(engine);
    engine.refresh().unwrap().0
}
fn ready(engine: &mut Engine) -> WorkspaceSnapshot {
    engine.refresh().unwrap();
    let p = engine.preview.as_ref().unwrap();
    if let Some(job) = &p.job {
        assert!(job.wait_timeout(Duration::from_secs(10)).is_some());
    }
    engine.refresh().unwrap().0
}

#[test]
fn snapshot_sequence_is_monotonic_and_idle_does_not_emit() {
    let mut engine = Engine::new(Default::default()).unwrap();
    let (first, changed) = engine.refresh().unwrap();
    assert!(changed);
    assert_eq!(engine.refresh().unwrap(), (first.clone(), false));
    let loaded = open(&mut engine, "bitmap-raw.psd");
    assert!(loaded.sequence.parse::<u64>().unwrap() > first.sequence.parse().unwrap());
    assert_eq!(loaded.documents.len(), 1);
    let done = ready(&mut engine);
    assert!(matches!(
        done.preview.unwrap().state,
        WorkspacePreviewState::Ready { .. }
    ));
    engine.refresh().unwrap();
    assert!(!engine.refresh().unwrap().1);
}

#[test]
fn switching_close_and_reload_never_return_another_revision_image() {
    let mut engine = Engine::new(Default::default()).unwrap();
    let first = open(&mut engine, "bitmap-raw.psd").documents[0].clone();
    ready(&mut engine);
    let held = engine.image(&first.id, &first.revision).unwrap();
    let second = open(&mut engine, "bitmap-rle.psd").documents[1].clone();
    ready(&mut engine);
    assert_eq!(
        engine.image(&first.id, &first.revision).unwrap_err().code,
        WorkspaceErrorCode::StalePreview
    );
    engine
        .act(WorkspaceAction::Close {
            document_id: second.id,
        })
        .unwrap();
    ready(&mut engine);
    assert_eq!(
        engine.image(&first.id, &first.revision).unwrap().png(),
        held.png()
    );
    engine
        .act(WorkspaceAction::Reload {
            document_id: first.id.clone(),
        })
        .unwrap();
    finish_opens(&engine);
    let reloaded = ready(&mut engine);
    assert_ne!(reloaded.documents[0].revision, first.revision);
    assert_eq!(
        engine.image(&first.id, &first.revision).unwrap_err().code,
        WorkspaceErrorCode::StalePreview
    );
    assert_eq!(&held.png()[..8], b"\x89PNG\r\n\x1a\n");
}

#[test]
fn failures_keep_open_documents_and_have_bounded_dismissible_notices() {
    let mut engine = Engine::new(Default::default()).unwrap();
    let first = open(&mut engine, "bitmap-raw.psd").documents[0].clone();
    ready(&mut engine);
    for _ in 0..18 {
        open(&mut engine, "does-not-exist.psd");
    }
    let snapshot = engine.refresh().unwrap().0;
    assert_eq!(snapshot.documents, vec![first]);
    assert_eq!(snapshot.notices.len(), 16);
    engine
        .act(WorkspaceAction::DismissNotice {
            notice_id: snapshot.notices[0].id.clone(),
        })
        .unwrap();
    assert_eq!(engine.refresh().unwrap().0.notices.len(), 15);
}

#[test]
fn malformed_requests_and_no_composite_remain_recoverable() {
    let mut engine = Engine::new(Default::default()).unwrap();
    assert_eq!(
        engine
            .act(WorkspaceAction::Open {
                path: "relative.psd".into()
            })
            .unwrap_err()
            .code,
        WorkspaceErrorCode::InvalidInput
    );
    assert_eq!(
        engine
            .act(WorkspaceAction::Activate {
                document_id: "invalid".into()
            })
            .unwrap_err()
            .code,
        WorkspaceErrorCode::NotFound
    );
    open(&mut engine, "bitmap-no-composite.psd");
    let snapshot = ready(&mut engine);
    assert_eq!(snapshot.documents.len(), 1);
    assert!(matches!(
        snapshot.preview.unwrap().state,
        WorkspacePreviewState::Failed { .. }
    ));
    assert_eq!(
        engine
            .act(WorkspaceAction::Cancel {
                job_id: "preview-99999".into()
            })
            .unwrap_err()
            .code,
        WorkspaceErrorCode::NotFound
    );
}

#[test]
fn close_releases_adapter_image_but_external_lease_stays_accounted() {
    let mut engine = Engine::new(Default::default()).unwrap();
    let doc = open(&mut engine, "bitmap-raw.psd").documents[0].clone();
    ready(&mut engine);
    let image = engine.image(&doc.id, &doc.revision).unwrap();
    engine
        .act(WorkspaceAction::Close {
            document_id: doc.id,
        })
        .unwrap();
    let closed = engine.refresh().unwrap().0;
    assert!(closed.documents.is_empty() && closed.preview.is_none());
    assert_eq!(closed.resources.source_bytes, "0");
    assert_eq!(closed.resources.cache_bytes, "0");
    assert_ne!(closed.resources.output_bytes, "0");
    drop(image);
    assert_eq!(engine.refresh().unwrap().0.resources.output_bytes, "0");
    engine.shutdown().unwrap();
    assert!(engine.refresh().unwrap().0.shutting_down);
}

#[test]
fn failed_reload_keeps_revision_and_visible_image() {
    struct Temporary(std::path::PathBuf);
    impl Drop for Temporary {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }
    let directory = Temporary(
        std::env::temp_dir().join(format!("layerlens-desktop-reload-{}", std::process::id())),
    );
    std::fs::create_dir(&directory.0).unwrap();
    let path = directory.0.join("reload.psd");
    std::fs::copy(fixture("bitmap-raw.psd"), &path).unwrap();
    let mut engine = Engine::new(Default::default()).unwrap();
    engine
        .act(WorkspaceAction::Open {
            path: path.to_string_lossy().into_owned(),
        })
        .unwrap();
    finish_opens(&engine);
    let first = ready(&mut engine).documents[0].clone();
    let image = engine.image(&first.id, &first.revision).unwrap();
    std::fs::write(&path, b"corrupted external revision").unwrap();
    engine
        .act(WorkspaceAction::Reload {
            document_id: first.id.clone(),
        })
        .unwrap();
    finish_opens(&engine);
    let failed = engine.refresh().unwrap().0;
    assert_eq!(failed.documents, vec![first.clone()]);
    assert_eq!(failed.notices.len(), 1);
    assert_eq!(
        engine.image(&first.id, &first.revision).unwrap().png(),
        image.png()
    );
}
