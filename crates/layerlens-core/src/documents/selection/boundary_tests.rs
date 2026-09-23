//! 错误和计数器极限注入；拒绝不得留下半提交或不可恢复的分页。

use super::*;

#[test]
fn exhausted_selection_version_cannot_partially_publish_any_active_transition() {
    let service = service(Default::default());
    let a = open(&service, "bitmap-raw.psd");
    let b = open(&service, "viewer-scale.psd");
    let selected = select(&service, &b, layers(&b, &[0]));
    lock(&service.shared.state).selections.revision = SelectionRevision(u64::MAX);
    let before = service.user_selection().unwrap();
    let usage = service.selection_resources();
    let source_usage = service.snapshot().resources;
    let pending = prepare(&service, &b, layers(&b, &[1]));
    assert!(matches!(
        service.commit_selection(pending),
        Err(SelectionError::Document(DocumentError::IdExhausted))
    ));
    assert!(matches!(
        service.clear_selection(&b, before.revision),
        Err(SelectionError::Document(DocumentError::IdExhausted))
    ));
    assert!(matches!(
        service.activate(a.document_id()),
        Err(DocumentError::IdExhausted)
    ));
    assert!(matches!(
        service.close(b.document_id()),
        Err(DocumentError::IdExhausted)
    ));
    let cleanup = service.prepare_cleanup(b.document_id()).unwrap();
    assert!(matches!(
        service.commit_cleanup(&cleanup),
        Err(DocumentError::IdExhausted)
    ));
    assert!(b.ensure_valid().is_ok());
    assert!(snapshot(&selected).content_page(None, 1).is_ok());
    let reload = service
        .reload(b.document_id())
        .unwrap()
        .wait_timeout(DEADLINE)
        .unwrap();
    assert!(
        matches!(&reload.outcome, OpenOutcome::Failed(error) if matches!(error.as_ref(), DocumentError::IdExhausted))
    );
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/psd/viewer-text.psd");
    let job = service.open(path).unwrap().wait_timeout(DEADLINE).unwrap();
    assert!(
        matches!(&job.outcome, OpenOutcome::Failed(error) if matches!(error.as_ref(), DocumentError::IdExhausted))
    );
    assert_eq!(
        service.lease(b.document_id()).unwrap().revision_id(),
        b.revision_id()
    );
    assert_eq!(service.snapshot().documents.len(), 2);
    assert_eq!(service.snapshot().resources, source_usage);
    assert_eq!(service.selection_resources(), usage);
    let after = service.user_selection().unwrap();
    assert_eq!(after.revision, before.revision);
    assert_eq!(after.document_id, Some(b.document_id()));
    assert_eq!(snapshot(&after).id(), snapshot(&selected).id());
    // 无实际变化仍可成功，不因版本到顶破坏幂等。
    service.activate(b.document_id()).unwrap();
    assert_eq!(
        select(&service, &b, layers(&b, &[0])).revision,
        before.revision
    );
}

#[test]
fn exhausted_snapshot_and_task_ids_do_not_consume_versions_or_request_keys() {
    let service = service(Default::default());
    let lease = open(&service, "viewer-scale.psd");
    let selected = select(&service, &lease, layers(&lease, &[0]));
    let usage = service.selection_resources();
    lock(&service.shared.state).selections.next_snapshot = u64::MAX;
    assert!(matches!(
        service.commit_selection(prepare(&service, &lease, layers(&lease, &[1]))),
        Err(SelectionError::Document(DocumentError::IdExhausted))
    ));
    assert_eq!(
        service.user_selection().unwrap().revision,
        selected.revision
    );
    assert_eq!(service.selection_resources(), usage);
    lock(&service.shared.state).selections.next_task = u64::MAX;
    assert!(matches!(
        service.create_task(
            &selected.session_id,
            "not-consumed",
            snapshot(&selected).id(),
            "超限"
        ),
        Err(SelectionError::Document(DocumentError::IdExhausted))
    ));
    let state = lock(&service.shared.state);
    assert!(state.selections.requests.is_empty());
    assert!(state.selections.tasks.is_empty());
}

#[test]
fn byte_limited_pages_reconstruct_every_layer_without_skips_or_duplicates() {
    let service = service(SelectionConfig {
        content_page_bytes: 1024,
        ..Default::default()
    });
    let lease = open(&service, "viewer-scale.psd");
    let selected = select(&service, &lease, full_region(&lease));
    let mut cursor = None;
    let mut found = Vec::new();
    loop {
        let page = snapshot(&selected)
            .content_page(cursor.as_ref(), 128)
            .unwrap();
        assert!(serde_json::to_vec(&page).unwrap().len() <= 1024);
        assert!(!page.layers.is_empty());
        assert_eq!(page.layer_count, 512);
        found.extend(page.layers.iter().map(|layer| layer.details.layer.id));
        assert!(found.len() <= 512);
        assert_eq!(page.truncated, page.next_cursor.is_some());
        if let Some(next) = page.next_cursor {
            assert_ne!(cursor.as_ref(), Some(&next));
            cursor = Some(next);
        } else {
            break;
        }
    }
    assert_eq!(
        found,
        lease
            .info()
            .layers
            .iter()
            .map(|layer| layer.id)
            .collect::<Vec<_>>()
    );
}

#[test]
fn unpageable_metadata_reports_limit_instead_of_truncating_original_style() {
    let service = service(SelectionConfig {
        content_page_bytes: 1024,
        ..Default::default()
    });
    let lease = open(&service, "viewer-text.psd");
    let layer = lease
        .info()
        .layers
        .iter()
        .find(|layer| layer.name == "large-styles")
        .unwrap();
    let selected = select(&service, &lease, layers(&lease, &[layer.id.0]));
    assert!(matches!(
        snapshot(&selected).content_page(None, 128),
        Err(SelectionError::Document(
            DocumentError::ResourceLimit { .. }
        ))
    ));
    assert_eq!(
        service.user_selection().unwrap().revision,
        selected.revision
    );
}

#[test]
fn invalid_cursors_and_limits_are_rejected_without_affecting_the_snapshot() {
    let service = service(Default::default());
    let lease = open(&service, "viewer-scale.psd");
    let first = select(&service, &lease, layers(&lease, &[0, 1]));
    let other = select(&service, &lease, layers(&lease, &[2]));
    let cursor = snapshot(&first)
        .content_page(None, 1)
        .unwrap()
        .next_cursor
        .unwrap();
    assert!(snapshot(&other).content_page(Some(&cursor), 1).is_err());
    for limit in [0, 129] {
        assert!(snapshot(&first).content_page(None, limit).is_err());
    }
    for (record, text_start) in [(3, 0), (2, 1), (0, 1)] {
        let mut invalid = cursor.clone();
        invalid.record = record;
        invalid.text_start = text_start;
        assert!(snapshot(&first).content_page(Some(&invalid), 1).is_err());
    }
    let lease = open(&service, "viewer-text.psd");
    let layer = lease
        .info()
        .layers
        .iter()
        .find(|layer| layer.name == "long-text")
        .unwrap();
    let text = &layer.text.as_ref().unwrap().raw_text;
    let split_surrogate = text.split('😀').next().unwrap().encode_utf16().count() as u32 + 1;
    let selected = select(&service, &lease, layers(&lease, &[layer.id.0]));
    let mut cursor = ContentCursor {
        session_id: selected.session_id.clone(),
        snapshot_id: snapshot(&selected).id(),
        record: 0,
        text_start: split_surrogate,
    };
    assert!(matches!(
        snapshot(&selected).content_page(Some(&cursor), 1),
        Err(SelectionError::Document(DocumentError::InvalidQuery(_)))
    ));
    cursor.text_start = u32::MAX;
    assert!(snapshot(&selected).content_page(Some(&cursor), 1).is_err());
    assert_eq!(
        service.user_selection().unwrap().revision,
        selected.revision
    );
    assert!(
        !snapshot(&selected)
            .content_page(None, 1)
            .unwrap()
            .layers
            .is_empty()
    );
}

#[test]
fn invalid_configuration_is_rejected_before_starting_a_worker() {
    for selection in [
        SelectionConfig {
            max_items: 0,
            ..Default::default()
        },
        SelectionConfig {
            max_scope_layers: 0,
            ..Default::default()
        },
        SelectionConfig {
            max_live_snapshots: 0,
            ..Default::default()
        },
        SelectionConfig {
            max_scope_bytes: 0,
            ..Default::default()
        },
        SelectionConfig {
            max_task_records: 0,
            ..Default::default()
        },
        SelectionConfig {
            content_page_bytes: 1023,
            ..Default::default()
        },
        SelectionConfig {
            content_page_bytes: 256 * 1024 + 1,
            ..Default::default()
        },
    ] {
        assert!(matches!(
            DocumentService::new(DocumentServiceConfig {
                selection,
                ..Default::default()
            }),
            Err(DocumentError::InvalidConfiguration(_))
        ));
    }
}
