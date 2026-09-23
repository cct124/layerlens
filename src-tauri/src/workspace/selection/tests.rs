//! 桌面选择、分页及任务闭环；真实可复现样本，不依赖窗口或固定休眠。
use super::*;
use layerlens_core::{
    IPC_PROTOCOL_VERSION,
    documents::{DocumentLease, DocumentService, DocumentServiceConfig},
    psd::LayerKind,
};
use std::{path::Path, time::Duration};

fn open(service: &DocumentService, file: &str) -> DocumentLease {
    let job = service
        .open(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../crates/layerlens-core/tests/fixtures/psd")
                .join(file),
        )
        .unwrap();
    assert!(job.wait_timeout(Duration::from_secs(10)).is_some());
    let state = service.snapshot();
    service.lease(state.active_document.unwrap()).unwrap()
}
fn request(session: &SessionId, operation: SelectionOperation) -> SelectionRequest {
    SelectionRequest {
        protocol_version: IPC_PROTOCOL_VERSION,
        session_id: session.as_str().to_owned(),
        operation,
    }
}
fn layers(service: &DocumentService, lease: &DocumentLease) -> SelectionOperation {
    let id = lease
        .info()
        .layers
        .iter()
        .find(|layer| layer.effective_visible && layer.kind != LayerKind::Group)
        .unwrap()
        .id
        .0;
    SelectionOperation::Layers {
        document_id: lease.document_id().get().to_string(),
        document_revision: lease.revision_id().get().to_string(),
        expected_revision: service.user_selection().unwrap().revision.get().to_string(),
        layer_ids: vec![id, id],
    }
}
fn bind(service: &DocumentService, session: &SessionId, lease: &DocumentLease) -> TaskDto {
    execute(
        &service.selection_handle(),
        request(session, layers(service, lease)),
        RESPONSE_BYTES,
    )
    .unwrap();
    let snapshot = service.user_selection().unwrap().snapshot.unwrap();
    match execute(
        &service.selection_handle(),
        request(
            session,
            SelectionOperation::CreateTask {
                snapshot_id: snapshot.id().get().to_string(),
                request_id: "intent".into(),
                name: "任务".into(),
            },
        ),
        RESPONSE_BYTES,
    )
    .unwrap()
    {
        SelectionReply::Task { task, .. } => task,
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn cleanup_terminal_state_and_errors_survive_the_desktop_contract() {
    let service = DocumentService::new(Default::default()).unwrap();
    let lease = open(&service, "viewer-text.psd");
    let session = service.user_selection().unwrap().session_id;
    let task = bind(&service, &session, &lease);
    service
        .commit_cleanup(&service.prepare_cleanup(lease.document_id()).unwrap())
        .unwrap();
    let handle = service.selection_handle();
    let error = execute(
        &handle,
        request(
            &session,
            SelectionOperation::Content {
                target: SelectionTarget::Task {
                    task_id: task.id.clone(),
                },
                cursor: None,
                limit: 16,
            },
        ),
        RESPONSE_BYTES,
    )
    .unwrap_err();
    assert_eq!(error.code, WorkspaceErrorCode::TaskInvalidated);
    assert_eq!(
        serde_json::to_value(&error).unwrap()["code"],
        "TASK_INVALIDATED"
    );
    for operation in [
        SelectionOperation::ReleaseTask {
            task_id: task.id.clone(),
        },
        SelectionOperation::CreateTask {
            snapshot_id: task.scope.snapshot_id.clone(),
            request_id: "intent".into(),
            name: "任务".into(),
        },
    ] {
        let reply = execute(&handle, request(&session, operation), RESPONSE_BYTES).unwrap();
        assert_eq!(
            serde_json::to_value(&reply).unwrap()["task"]["status"],
            "invalidated"
        );
    }
    let reply = execute(
        &handle,
        request(
            &session,
            SelectionOperation::Tasks {
                after: None,
                limit: 32,
            },
        ),
        RESPONSE_BYTES,
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(reply).unwrap()["page"]["tasks"][0]["status"],
        "invalidated"
    );
    assert_eq!(
        domain_error(&layerlens_core::documents::DocumentError::Invalidated).code,
        WorkspaceErrorCode::DocumentInvalidated
    );
    assert_eq!(
        domain_error(&layerlens_core::documents::DocumentError::StaleCleanupPlan).code,
        WorkspaceErrorCode::StaleCleanupPlan
    );
}

#[test]
fn desktop_region_is_authoritative_idempotent_and_can_bind_a_task() {
    let service = DocumentService::new(Default::default()).unwrap();
    let lease = open(&service, "viewer-text.psd");
    let session = service.user_selection().unwrap().session_id;
    let handle = service.selection_handle();
    let bounds = SelectionBounds {
        x: 0.0,
        y: 0.0,
        width: f64::from(lease.info().width),
        height: f64::from(lease.info().height),
    };
    let region = || SelectionOperation::Region {
        document_id: lease.document_id().get().to_string(),
        document_revision: lease.revision_id().get().to_string(),
        expected_revision: service.user_selection().unwrap().revision.get().to_string(),
        bounds,
    };
    execute(&handle, request(&session, region()), RESPONSE_BYTES).unwrap();
    let before = dto::summary(service.snapshot().selection);
    assert_eq!(before.region, Some(bounds));
    assert!(before.layer_ids.is_empty());
    let scope = before.scope.unwrap();
    assert!(scope.target_count > 0);
    let stale = region();
    execute(&handle, request(&session, region()), RESPONSE_BYTES).unwrap();
    let unchanged = dto::summary(service.snapshot().selection);
    assert_eq!(unchanged.selection_revision, before.selection_revision);
    assert_eq!(unchanged.scope.unwrap().snapshot_id, scope.snapshot_id);
    let task = match execute(
        &handle,
        request(
            &session,
            SelectionOperation::CreateTask {
                snapshot_id: scope.snapshot_id.clone(),
                request_id: "region-task".into(),
                name: "区域任务".into(),
            },
        ),
        RESPONSE_BYTES,
    )
    .unwrap()
    {
        SelectionReply::Task { task, .. } => task,
        other => panic!("unexpected {other:?}"),
    };
    execute(
        &handle,
        request(&session, layers(&service, &lease)),
        RESPONSE_BYTES,
    )
    .unwrap();
    assert!(dto::summary(service.snapshot().selection).region.is_none());
    assert_eq!(
        execute(&handle, request(&session, stale), RESPONSE_BYTES)
            .unwrap_err()
            .code,
        WorkspaceErrorCode::StaleSelection
    );
    service.close(lease.document_id()).unwrap();
    match execute(
        &handle,
        request(
            &session,
            SelectionOperation::Content {
                target: SelectionTarget::Task { task_id: task.id },
                cursor: None,
                limit: 16,
            },
        ),
        RESPONSE_BYTES,
    )
    .unwrap()
    {
        SelectionReply::Content { page, .. } => {
            assert_eq!(page.snapshot_id, scope.snapshot_id);
            assert!(!page.layers.is_empty());
            assert!(page.layers.iter().any(|layer| layer.intersection.is_some()));
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn malformed_outside_and_old_document_regions_preserve_existing_selection() {
    let service = DocumentService::new(Default::default()).unwrap();
    let lease = open(&service, "bitmap-raw.psd");
    let session = service.user_selection().unwrap().session_id;
    let handle = service.selection_handle();
    execute(
        &handle,
        request(&session, layers(&service, &lease)),
        RESPONSE_BYTES,
    )
    .unwrap();
    let old = service.user_selection().unwrap();
    let region = |bounds| SelectionOperation::Region {
        document_id: lease.document_id().get().to_string(),
        document_revision: lease.revision_id().get().to_string(),
        expected_revision: old.revision.get().to_string(),
        bounds,
    };
    for bounds in [
        SelectionBounds {
            x: -1.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        },
        SelectionBounds {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 1.0,
        },
        SelectionBounds {
            x: 0.0,
            y: 0.0,
            width: f64::from(lease.info().width) + 1.0,
            height: 1.0,
        },
        SelectionBounds {
            x: f64::NAN,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        },
    ] {
        assert_eq!(
            execute(&handle, request(&session, region(bounds)), RESPONSE_BYTES)
                .unwrap_err()
                .code,
            WorkspaceErrorCode::InvalidInput
        );
        assert_eq!(service.user_selection().unwrap().revision, old.revision);
    }
    let valid = region(SelectionBounds {
        x: 0.0,
        y: 0.0,
        width: 1.0,
        height: 1.0,
    });
    assert_eq!(
        execute(&handle, request(&session, valid.clone()), RECEIPT_BYTES - 1)
            .unwrap_err()
            .code,
        WorkspaceErrorCode::ResourceLimit
    );
    let mut json = serde_json::to_value(request(&session, valid.clone())).unwrap();
    json["operation"]["bounds"]["unexpected"] = true.into();
    assert!(serde_json::from_value::<SelectionRequest>(json).is_err());
    service
        .reload(lease.document_id())
        .unwrap()
        .wait_timeout(Duration::from_secs(10))
        .unwrap();
    assert!(execute(&handle, request(&session, valid), RESPONSE_BYTES).is_err());
    assert!(service.user_selection().unwrap().snapshot.is_none());
}

#[test]
fn desktop_binding_keeps_scope_after_reselection_reload_close_and_release() {
    let service = DocumentService::new(Default::default()).unwrap();
    let lease = open(&service, "viewer-text.psd");
    let session = service.user_selection().unwrap().session_id;
    let task = bind(&service, &session, &lease);
    let current = dto::summary(service.snapshot().selection);
    assert_eq!(current.layer_ids.len(), 1);
    assert_eq!(service.snapshot().documents[0].selected_layer, None);
    service
        .reload(lease.document_id())
        .unwrap()
        .wait_timeout(Duration::from_secs(10))
        .unwrap();
    assert!(service.user_selection().unwrap().snapshot.is_none());
    service.close(lease.document_id()).unwrap();
    let newer = open(&service, "bitmap-raw.psd");
    execute(
        &service.selection_handle(),
        request(&session, layers(&service, &newer)),
        RESPONSE_BYTES,
    )
    .unwrap();
    let read = || {
        request(
            &session,
            SelectionOperation::Content {
                target: SelectionTarget::Task {
                    task_id: task.id.clone(),
                },
                cursor: None,
                limit: 16,
            },
        )
    };
    match execute(&service.selection_handle(), read(), RESPONSE_BYTES).unwrap() {
        SelectionReply::Content { page, .. } => {
            assert_eq!(page.snapshot_id, task.scope.snapshot_id);
            assert_eq!(
                page.document_revision,
                lease.revision_id().get().to_string()
            );
            assert!(!page.layers.is_empty());
        }
        other => panic!("unexpected {other:?}"),
    }
    execute(
        &service.selection_handle(),
        request(
            &session,
            SelectionOperation::ReleaseTask {
                task_id: task.id.clone(),
            },
        ),
        RESPONSE_BYTES,
    )
    .unwrap();
    assert_eq!(
        execute(&service.selection_handle(), read(), RESPONSE_BYTES)
            .unwrap_err()
            .code,
        WorkspaceErrorCode::TaskReleased
    );
    let record = service
        .design_task(&session, task.id.parse().unwrap())
        .unwrap();
    assert_eq!(record.scope.revision_id, lease.revision_id());
    assert_eq!(record.status, TaskStatus::Released);
}

#[test]
fn stale_foreign_malformed_and_unacknowledgeable_changes_have_no_side_effects() {
    let service = DocumentService::new(Default::default()).unwrap();
    let lease = open(&service, "viewer-text.psd");
    let selection = service.user_selection().unwrap();
    let original = layers(&service, &lease);
    let handle = service.selection_handle();
    assert_eq!(
        execute(
            &handle,
            request(&selection.session_id, original.clone()),
            RECEIPT_BYTES - 1
        )
        .unwrap_err()
        .code,
        WorkspaceErrorCode::ResourceLimit
    );
    assert!(service.user_selection().unwrap().snapshot.is_none());
    for value in [
        "",
        "01",
        "-1",
        "+1",
        " 1",
        "18446744073709551616",
        "1e2",
        "１２",
    ] {
        assert!(value.parse::<SnapshotId>().is_err());
        assert!(value.parse::<SelectionRevision>().is_err());
    }
    assert!("0".parse::<SnapshotId>().is_err());
    assert!("0".parse::<SelectionRevision>().is_ok());
    assert!(DocumentReference::parse("01", "1").is_err());
    assert!("A".repeat(32).parse::<SessionId>().is_err());
    let foreign: SessionId = "0".repeat(32).parse().unwrap();
    assert_eq!(
        execute(&handle, request(&foreign, original.clone()), RESPONSE_BYTES)
            .unwrap_err()
            .code,
        WorkspaceErrorCode::ForeignSession
    );
    execute(
        &handle,
        request(&selection.session_id, original.clone()),
        RESPONSE_BYTES,
    )
    .unwrap();
    assert_eq!(
        execute(
            &handle,
            request(&selection.session_id, original),
            RESPONSE_BYTES
        )
        .unwrap_err()
        .code,
        WorkspaceErrorCode::StaleSelection
    );
    let snapshot = service.user_selection().unwrap().snapshot.unwrap();
    let create = SelectionOperation::CreateTask {
        snapshot_id: snapshot.id().get().to_string(),
        request_id: "not-accepted".into(),
        name: "task".into(),
    };
    assert!(execute(&handle, request(&selection.session_id, create.clone()), 1).is_err());
    assert_eq!(
        handle
            .list_tasks(&selection.session_id, None, 32)
            .unwrap()
            .total,
        0
    );
    let first = execute(
        &handle,
        request(&selection.session_id, create.clone()),
        RESPONSE_BYTES,
    )
    .unwrap();
    let duplicate = execute(
        &handle,
        request(&selection.session_id, create),
        RESPONSE_BYTES,
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(first).unwrap(),
        serde_json::to_value(duplicate).unwrap()
    );
    assert_eq!(
        handle
            .list_tasks(&selection.session_id, None, 32)
            .unwrap()
            .total,
        1
    );
}

#[test]
fn task_pages_retain_terminal_scope_and_capacity_without_shifting_cursors() {
    let service = DocumentService::new(DocumentServiceConfig {
        selection: SelectionConfig {
            max_task_records: 3,
            ..Default::default()
        },
        ..Default::default()
    })
    .unwrap();
    let lease = open(&service, "bitmap-raw.psd");
    let session = service.user_selection().unwrap().session_id;
    let first = bind(&service, &session, &lease);
    let handle = service.selection_handle();
    for i in 1..3 {
        handle
            .create_task(
                &session,
                &format!("req-{i}"),
                first.scope.snapshot_id.parse().unwrap(),
                "same-name",
            )
            .unwrap();
    }
    let page = handle.list_tasks(&session, None, 2).unwrap();
    handle.release_task(&session, page.tasks[0].id).unwrap();
    service.close(lease.document_id()).unwrap();
    let next = handle.list_tasks(&session, page.next_after, 2).unwrap();
    assert_eq!(next.tasks.len(), 1);
    assert_eq!(next.total, 3);
    assert_eq!(next.capacity, 3);
    assert!(next.next_after.is_none());
    assert_eq!(
        handle.list_tasks(&session, None, 1).unwrap().tasks[0]
            .scope
            .document_id,
        lease.document_id()
    );
    assert!(
        handle
            .create_task(
                &session,
                "overflow",
                first.scope.snapshot_id.parse().unwrap(),
                "same-name"
            )
            .is_err()
    );
    assert!(
        handle
            .list_tasks(&session, Some("999".parse().unwrap()), 2)
            .is_err()
    );
    assert!(handle.list_tasks(&session, None, 33).is_err());
}

#[test]
fn content_outer_budget_and_round_trip_cursors_preserve_utf16_text() {
    let service = DocumentService::new(Default::default()).unwrap();
    let lease = open(&service, "viewer-text.psd");
    let session = service.user_selection().unwrap().session_id;
    let task = bind(&service, &session, &lease);
    let handle = service.selection_handle();
    let mut cursor = None;
    let mut text = String::new();
    // 此样本的不可分页样式元数据不能放入 3 KiB 内容空间；必须拒绝，不能丢样式。
    assert_eq!(
        execute(
            &handle,
            request(
                &session,
                SelectionOperation::Content {
                    target: SelectionTarget::Task {
                        task_id: task.id.clone()
                    },
                    cursor: None,
                    limit: 16
                }
            ),
            4096
        )
        .unwrap_err()
        .code,
        WorkspaceErrorCode::ResourceLimit
    );
    let mut pages = 0;
    loop {
        let reply = execute(
            &handle,
            request(
                &session,
                SelectionOperation::Content {
                    target: SelectionTarget::Task {
                        task_id: task.id.clone(),
                    },
                    cursor,
                    limit: 16,
                },
            ),
            32 * 1024,
        )
        .unwrap();
        assert!(serde_json::to_vec(&reply).unwrap().len() <= 32 * 1024);
        pages += 1;
        let SelectionReply::Content { page, .. } = reply else {
            panic!("content expected")
        };
        for layer in page.layers {
            if let Some(part) = layer.details.text {
                assert_eq!(part.start as usize, text.encode_utf16().count());
                text.push_str(&part.text);
            }
        }
        cursor = page.next_cursor;
        if cursor.is_none() {
            break;
        }
    }
    let original = lease
        .info()
        .layers
        .iter()
        .find_map(|layer| layer.text.as_ref())
        .unwrap();
    assert_eq!(text, original.raw_text);
    assert!(pages > 1);
    let invalid = SelectionCursorDto {
        session_id: session.as_str().into(),
        snapshot_id: "9999".into(),
        record: 0,
        text_start: 0,
    };
    assert!(
        execute(
            &handle,
            request(
                &session,
                SelectionOperation::Content {
                    target: SelectionTarget::Task { task_id: task.id },
                    cursor: Some(invalid),
                    limit: 16
                }
            ),
            RESPONSE_BYTES
        )
        .is_err()
    );
}

#[test]
fn largest_mutation_receipt_fits_reserved_space() {
    let reply = SelectionReply::Task {
        session_id: "f".repeat(32),
        task: TaskDto {
            id: u64::MAX.to_string(),
            name: " ".repeat(256),
            status: TaskStatusDto::Released,
            scope: ScopeDto {
                snapshot_id: u64::MAX.to_string(),
                document_id: u64::MAX.to_string(),
                document_revision: u64::MAX.to_string(),
                document_name: " ".repeat(256),
                target_count: u32::MAX,
                content_count: u32::MAX,
                text_layer_count: u32::MAX,
            },
        },
    };
    check_size(&reply, RECEIPT_BYTES).unwrap();
}

#[test]
fn handle_rejects_work_after_owner_shutdown_and_summary_remains_atomic() {
    let mut service = DocumentService::new(Default::default()).unwrap();
    let first = open(&service, "bitmap-raw.psd");
    let second = open(&service, "bitmap-rle.psd");
    let handle = service.selection_handle();
    let session = service.user_selection().unwrap().session_id;
    std::thread::scope(|scope| {
        scope.spawn(|| {
            for _ in 0..50 {
                service.activate(first.document_id()).unwrap();
                service.activate(second.document_id()).unwrap();
            }
        });
        for _ in 0..100 {
            let state = service.snapshot();
            assert_eq!(state.active_document, state.selection.document_id);
            let active = state
                .documents
                .iter()
                .find(|doc| Some(doc.id) == state.active_document)
                .unwrap();
            assert_eq!(Some(active.revision), state.selection.document_revision);
        }
    });
    service.shutdown().unwrap();
    assert!(matches!(
        handle.list_tasks(&session, None, 1),
        Err(SelectionError::Document(
            layerlens_core::documents::DocumentError::ShuttingDown
        ))
    ));
}
