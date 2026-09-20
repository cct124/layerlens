//! 核心公开行为回归：固定范围、过期拒绝、幂等及最后引用释放；不依赖桌面或休眠。

use std::{path::Path, sync::Arc, time::Duration};

use super::*;
use crate::{
    documents::{DocumentServiceConfig, OpenOutcome, ResourceAccounting},
    psd::{LayerId, PsdDocument},
};

const DEADLINE: Duration = Duration::from_secs(10);

fn service(selection: SelectionConfig) -> DocumentService {
    DocumentService::with_loader(
        DocumentServiceConfig {
            selection,
            ..Default::default()
        },
        Box::new(|path, limits| PsdDocument::from_bytes(&std::fs::read(path).unwrap(), limits)),
    )
    .unwrap()
}

fn open(service: &DocumentService, name: &str) -> DocumentLease {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/psd")
        .join(name);
    let outcome = service.open(path).unwrap().wait_timeout(DEADLINE).unwrap();
    match outcome.outcome {
        OpenOutcome::Opened { document_id, .. } => service.lease(document_id).unwrap(),
        ref result => panic!("预期打开成功：{result:?}"),
    }
}

fn region(x: f64, y: f64, width: f64, height: f64) -> SelectionInput {
    SelectionInput::Region(SelectionBounds {
        x,
        y,
        width,
        height,
    })
}

fn layers(lease: &DocumentLease, ids: &[u32]) -> SelectionInput {
    SelectionInput::Layers(
        ids.iter()
            .map(|id| lease.selection_layer(LayerId(*id)).unwrap())
            .collect(),
    )
}

fn prepare(
    service: &DocumentService,
    lease: &DocumentLease,
    input: SelectionInput,
) -> PreparedSelection {
    service
        .prepare_selection(lease, service.user_selection().unwrap().revision, input)
        .unwrap()
}

fn select(
    service: &DocumentService,
    lease: &DocumentLease,
    input: SelectionInput,
) -> UserSelection {
    service
        .commit_selection(prepare(service, lease, input))
        .unwrap()
}

fn snapshot(selection: &UserSelection) -> &SnapshotLease {
    selection.snapshot.as_ref().unwrap()
}

fn full_region(lease: &DocumentLease) -> SelectionInput {
    region(
        0.0,
        0.0,
        f64::from(lease.info().width),
        f64::from(lease.info().height),
    )
}

#[test]
fn active_versions_change_only_for_effective_transitions() {
    let service = service(Default::default());
    let initial = service.user_selection().unwrap();
    assert_eq!(initial.revision.get(), 0);
    assert!(initial.document_id.is_none() && initial.snapshot.is_none());
    let a = open(&service, "viewer-scale.psd");
    assert_eq!(service.user_selection().unwrap().revision.get(), 1);
    let selected = select(&service, &a, layers(&a, &[0, 1, 0]));
    assert_eq!(snapshot(&selected).scope().items.len(), 2);
    let unchanged = select(&service, &a, layers(&a, &[1, 0]));
    assert_eq!(unchanged.revision, selected.revision);
    assert_eq!(snapshot(&unchanged).id(), snapshot(&selected).id());
    service.activate(a.document_id()).unwrap();
    open(&service, "viewer-scale.psd");
    service.select_layer(&a, Some(LayerId(4))).unwrap();
    assert_eq!(
        service.user_selection().unwrap().revision,
        selected.revision
    );
    let b = open(&service, "bitmap-raw.psd");
    let b_selection = service.user_selection().unwrap();
    assert!(b_selection.revision > selected.revision && b_selection.snapshot.is_none());
    service.activate(a.document_id()).unwrap();
    let restored = service.user_selection().unwrap();
    assert!(restored.revision > b_selection.revision);
    assert_eq!(snapshot(&restored).id(), snapshot(&selected).id());
    service
        .reload(b.document_id())
        .unwrap()
        .wait_timeout(DEADLINE)
        .unwrap();
    service.close(b.document_id()).unwrap();
    assert_eq!(
        service.user_selection().unwrap().revision,
        restored.revision
    );
    service.close(a.document_id()).unwrap();
    let empty = service.user_selection().unwrap();
    assert_eq!(empty.revision.get(), restored.revision.get() + 1);
    assert!(
        empty.document_id.is_none()
            && empty.document_revision.is_none()
            && empty.snapshot.is_none()
    );
}

#[test]
fn invalid_or_late_selection_does_not_replace_committed_scope() {
    let service = service(Default::default());
    let lease = open(&service, "viewer-scale.psd");
    let committed = select(&service, &lease, layers(&lease, &[0]));
    let late = prepare(&service, &lease, layers(&lease, &[1]));
    let new = select(&service, &lease, layers(&lease, &[2]));
    assert!(matches!(
        service.commit_selection(late),
        Err(SelectionError::StaleSelection)
    ));
    for input in [
        region(f64::NAN, 0.0, 1.0, 1.0),
        region(0.0, 0.0, f64::INFINITY, 1.0),
        region(-1.0, 0.0, 2.0, 1.0),
        region(0.0, 0.0, 0.0, 1.0),
        region(255.5, 0.0, 1.0, 1.0),
        SelectionInput::Layers(vec![]),
    ] {
        assert!(
            service
                .prepare_selection(&lease, new.revision, input)
                .is_err()
        );
    }
    assert_eq!(
        snapshot(&service.user_selection().unwrap()).id(),
        snapshot(&new).id()
    );
    assert_ne!(snapshot(&committed).id(), snapshot(&new).id());
    let empty = service.clear_selection(&lease, new.revision).unwrap();
    assert!(empty.snapshot.is_none());
    assert_eq!(empty.revision.get(), new.revision.get() + 1);
    assert_eq!(
        service
            .clear_selection(&lease, empty.revision)
            .unwrap()
            .revision,
        empty.revision
    );
}

#[test]
fn switch_away_and_back_invalidates_prepared_edit_even_with_same_document() {
    let service = service(Default::default());
    let a = open(&service, "viewer-scale.psd");
    let pending = prepare(&service, &a, layers(&a, &[0]));
    let b = open(&service, "bitmap-raw.psd");
    assert!(matches!(
        service.prepare_selection(
            &a,
            service.user_selection().unwrap().revision,
            layers(&a, &[0])
        ),
        Err(SelectionError::InactiveDocument)
    ));
    service.activate(a.document_id()).unwrap();
    assert!(matches!(
        service.commit_selection(pending),
        Err(SelectionError::StaleSelection)
    ));
    let reference = b.selection_layer(LayerId(0)).unwrap();
    assert!(matches!(
        service.prepare_selection(
            &a,
            service.user_selection().unwrap().revision,
            SelectionInput::Layers(vec![reference])
        ),
        Err(SelectionError::InvalidInput(_))
    ));
}

#[test]
fn regions_exclude_edge_contacts_preserve_fractional_intersections_and_order() {
    let service = service(Default::default());
    let lease = open(&service, "viewer-scale.psd");
    let selection = select(&service, &lease, region(0.5, 0.25, 7.5, 7.75));
    let scope = snapshot(&selection).scope();
    assert_eq!(scope.items.len(), 1);
    assert_eq!(scope.target_layer_ids.len(), 1);
    assert!(scope.selected_group_ids.is_empty());
    let page = snapshot(&selection).content_page(None, 128).unwrap();
    assert_eq!(page.layer_count, 1);
    assert_eq!(page.layers[0].details.layer.name, "tile-000");
    assert_eq!(
        page.layers[0].details.layer.bounds,
        crate::psd::Bounds {
            x: 0,
            y: 0,
            width: 8,
            height: 8
        }
    );
    let hit = page.layers[0].intersection.unwrap();
    assert_eq!(hit.kind, IntersectionKind::Partial);
    assert_eq!(
        hit.bounds,
        SelectionBounds {
            x: 0.5,
            y: 0.25,
            width: 7.5,
            height: 7.75
        }
    );
    assert!(
        page.warnings
            .contains(&SelectionWarning::GeometricBoundsOnly)
    );
    let whole = select(&service, &lease, full_region(&lease));
    let mut cursor = None;
    let mut found = Vec::new();
    loop {
        let page = snapshot(&whole).content_page(cursor.as_ref(), 128).unwrap();
        assert_eq!(page.layer_count, 512);
        assert_eq!(page.truncated, page.next_cursor.is_some());
        assert!(serde_json::to_vec(&page).unwrap().len() <= 256 * 1024);
        for record in page.layers {
            assert_eq!(record.role, LayerRole::Target);
            assert_eq!(
                record.intersection.unwrap().kind,
                IntersectionKind::Contained
            );
            found.push(record.details.layer.id);
        }
        cursor = page.next_cursor;
        if cursor.is_none() {
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
fn hidden_or_empty_selection_cannot_bind_a_task() {
    let service = service(Default::default());
    let lease = open(&service, "resources-groups-alpha-raw.psd");
    let hidden = lease
        .info()
        .layers
        .iter()
        .find(|layer| layer.name == "hidden-folder")
        .unwrap();
    let selection = select(&service, &lease, layers(&lease, &[hidden.id.0]));
    assert!(snapshot(&selection).scope().target_layer_ids.is_empty());
    assert_eq!(
        snapshot(&selection).scope().selected_group_ids,
        vec![hidden.id]
    );
    assert!(matches!(
        service.create_task(
            &selection.session_id,
            "empty",
            snapshot(&selection).id(),
            "隐藏组"
        ),
        Err(SelectionError::EmptyTargets)
    ));
    let lease = open(&service, "viewer-text.psd");
    let empty = select(&service, &lease, region(0.0, 0.0, 1.0, 1.0));
    assert!(snapshot(&empty).scope().target_layer_ids.is_empty());
    assert!(!snapshot(&empty).content_page(None, 1).unwrap().truncated);
    let valid = select(&service, &lease, full_region(&lease));
    // 失败请求没有占用幂等键。
    assert!(
        service
            .create_task(
                &valid.session_id,
                "empty",
                snapshot(&valid).id(),
                "可见文字"
            )
            .is_ok()
    );
}

#[test]
fn task_binding_survives_changes_close_reload_and_source_revision_replacement() {
    let mut service = service(SelectionConfig {
        max_cached_snapshots: 0,
        ..Default::default()
    });
    let lease = open(&service, "viewer-scale.psd");
    let selected = select(&service, &lease, layers(&lease, &[511]));
    let task = service
        .create_task(
            &selected.session_id,
            "request-a",
            snapshot(&selected).id(),
            "模块 A",
        )
        .unwrap();
    let hash = snapshot(&selected).source_sha256().to_owned();
    let fixed = service
        .task_snapshot(&selected.session_id, task.id)
        .unwrap();
    let b = select(&service, &lease, layers(&lease, &[0]));
    service.clear_selection(&lease, b.revision).unwrap();
    service
        .reload(lease.document_id())
        .unwrap()
        .wait_timeout(DEADLINE)
        .unwrap();
    let current = service.lease(lease.document_id()).unwrap();
    assert_ne!(current.revision_id(), fixed.revision_id());
    assert_eq!(fixed.document_info().width, 256);
    assert_eq!(fixed.document_info().height, 128);
    assert_eq!(fixed.source_sha256(), hash);
    assert_eq!(
        fixed.content_page(None, 1).unwrap().layers[0]
            .details
            .layer
            .name,
        "tile-000"
    );
    service.close(lease.document_id()).unwrap();
    assert!(service.user_selection().unwrap().document_id.is_none());
    assert_eq!(
        service
            .task_snapshot(&selected.session_id, task.id)
            .unwrap()
            .id(),
        fixed.id()
    );
    let reopened = open(&service, "viewer-scale.psd");
    assert_ne!(reopened.document_id(), lease.document_id());
    assert_eq!(
        service
            .task_snapshot(&selected.session_id, task.id)
            .unwrap()
            .document_id(),
        lease.document_id()
    );
    let released = service.release_task(&selected.session_id, task.id).unwrap();
    assert_eq!(released.status, TaskStatus::Released);
    assert_eq!(
        service.release_task(&selected.session_id, task.id).unwrap(),
        released
    );
    assert!(matches!(
        service.task_snapshot(&selected.session_id, task.id),
        Err(SelectionError::TaskReleased)
    ));
    assert_eq!(
        service
            .create_task(
                &selected.session_id,
                "request-a",
                task.snapshot_id,
                "模块 A"
            )
            .unwrap(),
        released
    );
    drop((lease, current, reopened, selected, b));
    service.shutdown().unwrap();
    assert_eq!(service.selection_resources().live_snapshots, 1);
    assert_eq!(service.snapshot().resources.live_revisions, 1);
    assert_eq!(fixed.content_page(None, 1).unwrap().layer_count, 1);
    drop(fixed);
    assert_eq!(
        service.selection_resources(),
        SelectionAccounting::default()
    );
    assert_eq!(service.snapshot().resources, ResourceAccounting::default());
}

#[test]
fn creation_is_atomic_idempotent_and_keeps_bounded_terminal_records() {
    let service = service(SelectionConfig {
        max_task_records: 1,
        ..Default::default()
    });
    let lease = open(&service, "viewer-scale.psd");
    let selected = select(&service, &lease, layers(&lease, &[0]));
    let id = snapshot(&selected).id();
    let task = service
        .create_task(&selected.session_id, "one", id, "同名任务")
        .unwrap();
    assert_eq!(
        service
            .create_task(&selected.session_id, "one", id, "同名任务")
            .unwrap(),
        task
    );
    assert!(matches!(
        service.create_task(&selected.session_id, "one", id, "不同参数"),
        Err(SelectionError::RequestConflict)
    ));
    let next = select(&service, &lease, layers(&lease, &[1]));
    assert!(matches!(
        service.create_task(
            &selected.session_id,
            "one",
            snapshot(&next).id(),
            "同名任务"
        ),
        Err(SelectionError::RequestConflict)
    ));
    service.release_task(&selected.session_id, task.id).unwrap();
    assert!(matches!(
        service.create_task(&selected.session_id, "two", id, "同名任务"),
        Err(SelectionError::Document(
            DocumentError::ResourceLimit { .. }
        ))
    ));
    assert_eq!(
        service
            .design_task(&selected.session_id, task.id)
            .unwrap()
            .status,
        TaskStatus::Released
    );
}

#[test]
fn foreign_sessions_and_foreign_leases_never_resolve_local_ids() {
    let a_service = service(Default::default());
    let b_service = service(Default::default());
    let a = open(&a_service, "viewer-scale.psd");
    let b = open(&b_service, "viewer-scale.psd");
    assert_eq!(a.document_id(), b.document_id());
    let sa = select(&a_service, &a, layers(&a, &[0]));
    let sb = select(&b_service, &b, layers(&b, &[0]));
    assert_ne!(sa.session_id, sb.session_id);
    assert!(matches!(
        b_service.selection_snapshot(&sa.session_id, snapshot(&sa).id()),
        Err(SelectionError::ForeignSession)
    ));
    assert!(matches!(
        b_service.create_task(&sa.session_id, "foreign", snapshot(&sa).id(), "foreign"),
        Err(SelectionError::ForeignSession)
    ));
    assert!(matches!(
        b_service.prepare_selection(&a, sb.revision, layers(&a, &[0])),
        Err(SelectionError::Document(DocumentError::StaleRevision))
    ));
    let cursor = snapshot(&sa).content_page(None, 1).unwrap().next_cursor;
    assert!(cursor.is_none());
    let many = select(&a_service, &a, full_region(&a));
    let page = snapshot(&many).content_page(None, 1).unwrap();
    assert!(
        snapshot(&sb)
            .content_page(page.next_cursor.as_ref(), 1)
            .is_err()
    );
}

#[test]
fn snapshot_limits_fail_without_partial_commit_and_reclaim_only_cache() {
    let service = service(SelectionConfig {
        max_live_snapshots: 2,
        max_cached_snapshots: 1,
        ..Default::default()
    });
    let lease = open(&service, "viewer-scale.psd");
    let first = select(&service, &lease, layers(&lease, &[0]));
    let session = first.session_id.clone();
    let first_id = snapshot(&first).id();
    drop(first);
    let second = select(&service, &lease, layers(&lease, &[1]));
    assert!(service.selection_snapshot(&session, first_id).is_ok());
    // 第三个快照先淘汰无外部引用的旧缓存；当前选择继续受保护。
    let third = select(&service, &lease, layers(&lease, &[2]));
    assert!(matches!(
        service.selection_snapshot(&session, first_id),
        Err(SelectionError::SnapshotExpired)
    ));
    assert!(matches!(
        service.create_task(&session, "expired", first_id, "过期"),
        Err(SelectionError::SnapshotExpired)
    ));
    let pending = prepare(&service, &lease, layers(&lease, &[3]));
    assert!(matches!(
        service.commit_selection(pending),
        Err(SelectionError::Document(
            DocumentError::ResourceLimit { .. }
        ))
    ));
    assert_eq!(service.user_selection().unwrap().revision, third.revision);
    assert_eq!(
        snapshot(&service.user_selection().unwrap()).id(),
        snapshot(&third).id()
    );
    drop(second);
    select(&service, &lease, layers(&lease, &[3]));
    assert!(service.selection_resources().live_snapshots <= 2);
}

#[test]
fn scope_and_input_budgets_reject_before_changing_selection() {
    let service = service(SelectionConfig {
        max_items: 1,
        max_scope_layers: 2,
        max_scope_bytes: 1,
        ..Default::default()
    });
    let lease = open(&service, "viewer-scale.psd");
    let version = service.user_selection().unwrap().revision;
    assert!(
        service
            .prepare_selection(&lease, version, layers(&lease, &[0, 0]))
            .is_err()
    );
    assert!(
        service
            .prepare_selection(&lease, version, full_region(&lease))
            .is_err()
    );
    let pending = prepare(&service, &lease, layers(&lease, &[0]));
    assert!(service.commit_selection(pending).is_err());
    assert_eq!(service.user_selection().unwrap().revision, version);
    assert!(service.user_selection().unwrap().snapshot.is_none());
    assert_eq!(
        service.selection_resources(),
        SelectionAccounting::default()
    );
}

#[test]
fn old_revision_and_closed_document_reject_prepared_scope() {
    let service = service(Default::default());
    let old = open(&service, "viewer-scale.psd");
    let pending = prepare(&service, &old, layers(&old, &[0]));
    let before = service.user_selection().unwrap().revision;
    service
        .reload(old.document_id())
        .unwrap()
        .wait_timeout(DEADLINE)
        .unwrap();
    assert_eq!(
        service.user_selection().unwrap().revision.get(),
        before.get() + 1
    );
    assert!(matches!(
        service.commit_selection(pending),
        Err(SelectionError::Document(DocumentError::StaleRevision))
    ));
    let new = service.lease(old.document_id()).unwrap();
    let pending = prepare(&service, &new, layers(&new, &[0]));
    let old_ref = old.selection_layer(LayerId(0)).unwrap();
    assert!(
        service
            .prepare_selection(
                &new,
                service.user_selection().unwrap().revision,
                SelectionInput::Layers(vec![old_ref])
            )
            .is_err()
    );
    service.close(old.document_id()).unwrap();
    assert!(matches!(
        service.commit_selection(pending),
        Err(SelectionError::Document(DocumentError::NotFound(_)))
    ));
    assert_eq!(
        service.selection_resources(),
        SelectionAccounting::default()
    );
}

#[test]
fn concurrent_same_request_registers_only_one_task() {
    let service = Arc::new(service(Default::default()));
    let lease = open(&service, "viewer-scale.psd");
    let selected = select(&service, &lease, layers(&lease, &[0]));
    let id = snapshot(&selected).id();
    let barrier = Arc::new(std::sync::Barrier::new(4));
    let handles: Vec<_> = (0..4)
        .map(|_| {
            let service = Arc::clone(&service);
            let session = selected.session_id.clone();
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                service
                    .create_task(&session, "same-request", id, "模块")
                    .unwrap()
            })
        })
        .collect();
    let tasks: Vec<_> = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect();
    assert!(tasks.iter().all(|task| task == &tasks[0]));
    assert_eq!(lock(&service.shared.state).selections.tasks.len(), 1);
}

#[test]
fn region_text_pagination_reconstructs_full_original_text_and_absolute_styles() {
    let service = service(SelectionConfig {
        content_page_bytes: 48 * 1024,
        ..Default::default()
    });
    let lease = open(&service, "viewer-text.psd");
    // 部分经过文字也读取全文，区域不是字符范围。
    let selection = select(&service, &lease, region(16.5, 16.5, 1.0, 1.0));
    let task = service
        .create_task(
            &selection.session_id,
            "text",
            snapshot(&selection).id(),
            "文字",
        )
        .unwrap();
    let fixed = service
        .task_snapshot(&selection.session_id, task.id)
        .unwrap();
    let mut texts: BTreeMap<u32, String> = BTreeMap::new();
    let mut style_ranges: BTreeMap<u32, BTreeMap<(u32, u32), serde_json::Value>> = BTreeMap::new();
    let mut paragraph_ranges: BTreeMap<u32, BTreeMap<(u32, u32), serde_json::Value>> =
        BTreeMap::new();
    let mut cursor = None;
    let mut pages = 0;
    loop {
        let page = fixed.content_page(cursor.as_ref(), 128).unwrap();
        assert!(serde_json::to_vec(&page).unwrap().len() <= 48 * 1024);
        assert_eq!((page.layer_count, page.text_layer_count), (3, 3));
        assert_eq!(page.document_id, fixed.document_id());
        assert_eq!(page.document_revision, fixed.revision_id());
        for record in page.layers {
            assert_eq!(record.intersection.unwrap().kind, IntersectionKind::Partial);
            let original = lease
                .info()
                .layers
                .iter()
                .find(|layer| layer.id == record.details.layer.id)
                .unwrap()
                .text
                .as_ref()
                .unwrap();
            let text = record.details.text.unwrap();
            let reconstructed = texts.entry(record.details.layer.id.0).or_default();
            assert_eq!(text.start as usize, reconstructed.encode_utf16().count());
            assert_eq!(text.transform, original.transform);
            assert_eq!(text.total_length, original.utf16_length);
            reconstructed.push_str(&text.text);
            for run in text.style_runs.unwrap_or_default() {
                assert!(run.start < text.end && run.end > text.start);
                style_ranges
                    .entry(record.details.layer.id.0)
                    .or_default()
                    .insert((run.start, run.end), serde_json::to_value(run).unwrap());
            }
            for run in text.paragraph_runs.unwrap_or_default() {
                assert!(run.start < text.end && run.end > text.start);
                paragraph_ranges
                    .entry(record.details.layer.id.0)
                    .or_default()
                    .insert((run.start, run.end), serde_json::to_value(run).unwrap());
            }
        }
        pages += 1;
        assert!(pages < 1000);
        cursor = page.next_cursor;
        if cursor.is_none() {
            break;
        }
        // 续页和绑定任务都不使用最新选区。
        if pages == 1 {
            select(&service, &lease, region(0.0, 0.0, 1.0, 1.0));
        }
    }
    assert!(pages > 3);
    for layer in &lease.info().layers {
        let original = layer.text.as_ref().unwrap();
        assert_eq!(texts[&layer.id.0], original.raw_text);
        let expected: BTreeMap<_, _> = original
            .style_runs
            .as_ref()
            .unwrap()
            .iter()
            .map(|run| ((run.start, run.end), serde_json::to_value(run).unwrap()))
            .collect();
        assert_eq!(style_ranges[&layer.id.0], expected);
        let expected: BTreeMap<_, _> = original
            .paragraph_runs
            .as_ref()
            .unwrap()
            .iter()
            .map(|run| ((run.start, run.end), serde_json::to_value(run).unwrap()))
            .collect();
        assert_eq!(paragraph_ranges[&layer.id.0], expected);
    }
}

#[test]
fn shutdown_drops_internal_task_and_cache_references_but_not_inflight_reads() {
    let mut service = service(Default::default());
    let lease = open(&service, "viewer-scale.psd");
    let selected = select(&service, &lease, layers(&lease, &[0]));
    let task = service
        .create_task(
            &selected.session_id,
            "shutdown",
            snapshot(&selected).id(),
            "退出",
        )
        .unwrap();
    let fixed = service
        .task_snapshot(&selected.session_id, task.id)
        .unwrap();
    let session = selected.session_id.clone();
    select(&service, &lease, layers(&lease, &[1]));
    drop((selected, lease));
    service.shutdown().unwrap();
    service.shutdown().unwrap();
    assert!(matches!(
        service.task_snapshot(&session, task.id),
        Err(SelectionError::Document(DocumentError::ShuttingDown))
    ));
    assert_eq!(service.selection_resources().live_snapshots, 1);
    assert_eq!(fixed.content_page(None, 1).unwrap().layer_count, 1);
    drop(fixed);
    assert_eq!(
        service.selection_resources(),
        SelectionAccounting::default()
    );
    assert_eq!(service.snapshot().resources, ResourceAccounting::default());
}

#[path = "geometry_tests.rs"]
mod geometry;

#[path = "boundary_tests.rs"]
mod boundary;
