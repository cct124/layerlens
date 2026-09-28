use super::*;
use layerlens_core::{
    IPC_PROTOCOL_VERSION,
    documents::{
        DocumentLease, DocumentService,
        selection::{SelectionInput, TaskStatus},
    },
    psd::LayerKind,
};
use std::path::Path;

fn open(service: &DocumentService) -> DocumentLease {
    let job = service
        .open(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../crates/layerlens-core/tests/fixtures/psd/viewer-text.psd"),
        )
        .unwrap();
    job.wait_timeout(Duration::from_secs(10)).unwrap();
    service
        .lease(service.snapshot().active_document.unwrap())
        .unwrap()
}
fn select(service: &DocumentService, lease: &DocumentLease) {
    let layer = lease
        .info()
        .layers
        .iter()
        .find(|layer| layer.effective_visible && layer.kind != LayerKind::Group)
        .unwrap()
        .id;
    let prepared = service
        .prepare_selection(
            lease,
            service.user_selection().unwrap().revision,
            SelectionInput::Layers(vec![lease.selection_layer(layer).unwrap()]),
        )
        .unwrap();
    service.commit_selection(prepared).unwrap();
}
fn run(
    plans: &mut Plans,
    service: &DocumentService,
    operation: CleanupOperation,
    now: Instant,
) -> Result<CleanupReply, WorkspaceError> {
    plans.execute(
        &service.selection_handle(),
        &service.cleanup_handle(),
        CleanupRequest {
            protocol_version: IPC_PROTOCOL_VERSION,
            session_id: service.user_selection().unwrap().session_id.as_str().into(),
            operation,
        },
        now,
        RESPONSE_BYTES,
    )
}
fn target(lease: &DocumentLease) -> CleanupOperation {
    CleanupOperation::Prepare {
        target: CleanupTarget::Document {
            document_id: lease.document_id().get().to_string(),
            document_revision: lease.revision_id().get().to_string(),
        },
    }
}
fn prepared(reply: CleanupReply) -> CleanupPlanDto {
    let CleanupReply::Prepared { plan, .. } = reply else {
        panic!("expected plan")
    };
    plan
}
fn receipt(reply: CleanupReply) -> CleanupReceiptDto {
    let CleanupReply::Committed { receipt, .. } = reply else {
        panic!("expected receipt")
    };
    receipt
}

#[test]
fn closed_task_cleanup_preserves_reopened_document_and_replays_even_after_cancel() {
    let service = DocumentService::new(Default::default()).unwrap();
    let lease = open(&service);
    select(&service, &lease);
    let selection = service.user_selection().unwrap();
    let task = service
        .create_task(
            &selection.session_id,
            "intent",
            selection.snapshot.unwrap().id(),
            "原任务",
        )
        .unwrap();
    service.close(lease.document_id()).unwrap();
    let reopened = open(&service);
    let mut plans = Plans::default();
    let now = Instant::now();
    let plan = prepared(
        run(
            &mut plans,
            &service,
            CleanupOperation::Prepare {
                target: CleanupTarget::Task {
                    task_id: task.id.get().to_string(),
                },
            },
            now,
        )
        .unwrap(),
    );
    assert_eq!(plan.open_revision, None);
    assert_eq!(plan.tasks.len(), 1);
    assert!(lease.ensure_valid().is_ok());
    let first = receipt(
        run(
            &mut plans,
            &service,
            CleanupOperation::Confirm {
                plan_id: plan.plan_id.clone(),
            },
            now,
        )
        .unwrap(),
    );
    assert_eq!(first.invalidated_task_ids, vec![task.id.get().to_string()]);
    for operation in [
        CleanupOperation::Confirm {
            plan_id: plan.plan_id.clone(),
        },
        CleanupOperation::Cancel {
            plan_id: plan.plan_id,
        },
    ] {
        assert_eq!(
            receipt(run(&mut plans, &service, operation, now + PLAN_TTL).unwrap()),
            first
        );
    }
    assert_eq!(service.snapshot().documents[0].id, reopened.document_id());
    assert_eq!(service.snapshot().cleanup_revision, 1);
    assert_eq!(service.snapshot().invalidated_tasks, vec![task.id]);
    assert_eq!(
        service
            .design_task(&selection.session_id, task.id)
            .unwrap()
            .status,
        TaskStatus::Invalidated
    );
    assert!(lease.ensure_valid().is_err());
    assert!(reopened.ensure_valid().is_ok());
}

#[test]
fn cancel_expiry_and_capacity_are_bounded_and_side_effect_free() {
    let service = DocumentService::new(Default::default()).unwrap();
    let lease = open(&service);
    let mut plans = Plans::default();
    let now = Instant::now();
    let mut ids = Vec::new();
    for _ in 0..PLAN_LIMIT {
        ids.push(prepared(run(&mut plans, &service, target(&lease), now).unwrap()).plan_id);
    }
    assert_eq!(
        run(&mut plans, &service, target(&lease), now)
            .unwrap_err()
            .code,
        WorkspaceErrorCode::ResourceLimit
    );
    run(
        &mut plans,
        &service,
        CleanupOperation::Cancel {
            plan_id: ids[0].clone(),
        },
        now,
    )
    .unwrap();
    let next = prepared(run(&mut plans, &service, target(&lease), now).unwrap());
    assert!(!ids.contains(&next.plan_id));
    assert_eq!(
        run(
            &mut plans,
            &service,
            CleanupOperation::Confirm {
                plan_id: next.plan_id
            },
            now + PLAN_TTL
        )
        .unwrap_err()
        .code,
        WorkspaceErrorCode::CleanupPlanExpired
    );
    assert!(plans.pending.is_empty());
    assert_eq!(service.snapshot().cleanup_revision, 0);
    assert!(lease.ensure_valid().is_ok());
}

#[test]
fn stale_plan_requires_explicit_new_preflight() {
    let service = DocumentService::new(Default::default()).unwrap();
    let lease = open(&service);
    let mut plans = Plans::default();
    let now = Instant::now();
    let plan = prepared(run(&mut plans, &service, target(&lease), now).unwrap());
    select(&service, &lease);
    let operation = CleanupOperation::Confirm {
        plan_id: plan.plan_id,
    };
    assert_eq!(
        run(&mut plans, &service, operation.clone(), now)
            .unwrap_err()
            .code,
        WorkspaceErrorCode::StaleCleanupPlan
    );
    assert_eq!(
        run(&mut plans, &service, operation, now).unwrap_err().code,
        WorkspaceErrorCode::CleanupPlanExpired
    );
    assert!(lease.ensure_valid().is_ok());
    assert_eq!(service.snapshot().cleanup_revision, 0);
}

#[test]
fn foreign_session_protocol_response_budget_and_shutdown_reject_before_mutation() {
    let mut service = DocumentService::new(Default::default()).unwrap();
    let lease = open(&service);
    let mut plans = Plans::default();
    let now = Instant::now();
    let plan = prepared(run(&mut plans, &service, target(&lease), now).unwrap());
    let mut request = CleanupRequest {
        protocol_version: IPC_PROTOCOL_VERSION,
        session_id: "a".repeat(32),
        operation: CleanupOperation::Confirm {
            plan_id: plan.plan_id,
        },
    };
    let selection = service.selection_handle();
    let cleanup = service.cleanup_handle();
    assert_eq!(
        plans
            .execute(&selection, &cleanup, request.clone(), now, RESPONSE_BYTES)
            .unwrap_err()
            .code,
        WorkspaceErrorCode::ForeignSession
    );
    request.session_id = service.user_selection().unwrap().session_id.as_str().into();
    assert_eq!(
        plans
            .execute(
                &selection,
                &cleanup,
                request.clone(),
                now,
                RECEIPT_BYTES - 1
            )
            .unwrap_err()
            .code,
        WorkspaceErrorCode::ResourceLimit
    );
    request.protocol_version += 1;
    assert_eq!(
        plans
            .execute(&selection, &cleanup, request.clone(), now, RESPONSE_BYTES)
            .unwrap_err()
            .code,
        WorkspaceErrorCode::ProtocolMismatch
    );
    request.protocol_version = IPC_PROTOCOL_VERSION;
    assert!(lease.ensure_valid().is_ok());
    service.shutdown().unwrap();
    assert_eq!(
        plans
            .execute(&selection, &cleanup, request, now, RESPONSE_BYTES)
            .unwrap_err()
            .code,
        WorkspaceErrorCode::ShuttingDown
    );
}

#[test]
fn maximum_escaped_task_names_and_receipt_fit_budgets_and_all_tasks_are_projected() {
    let service = DocumentService::new(Default::default()).unwrap();
    let lease = open(&service);
    select(&service, &lease);
    let selection = service.user_selection().unwrap();
    let snapshot = selection.snapshot.unwrap();
    for id in 0..128 {
        service
            .create_task(
                &selection.session_id,
                &id.to_string(),
                snapshot.id(),
                &format!("x{}", "\u{1}".repeat(255)),
            )
            .unwrap();
    }
    let mut plans = Plans::default();
    let now = Instant::now();
    let reply = run(&mut plans, &service, target(&lease), now).unwrap();
    let bytes = serde_json::to_vec(&reply).unwrap().len();
    assert!(bytes > 190_000 && bytes <= RESPONSE_BYTES);
    let plan = prepared(reply);
    assert_eq!(plan.tasks.len(), 128);
    let reply = run(
        &mut plans,
        &service,
        CleanupOperation::Confirm {
            plan_id: plan.plan_id,
        },
        now,
    )
    .unwrap();
    assert!(serde_json::to_vec(&reply).unwrap().len() < RECEIPT_BYTES);
    assert_eq!(service.snapshot().invalidated_tasks.len(), 128);
}

#[test]
fn publication_checkpoint_rejects_results_computed_before_cleanup() {
    let service = DocumentService::new(Default::default()).unwrap();
    let lease = open(&service);
    let handle = service.cleanup_handle();
    validate_publication(&handle, lease.document_id(), lease.revision_id()).unwrap();
    service.close(lease.document_id()).unwrap();
    let reopened = open(&service);
    handle
        .commit_cleanup(&handle.prepare_cleanup(lease.document_id()).unwrap())
        .unwrap();
    assert_eq!(
        validate_publication(&handle, lease.document_id(), lease.revision_id())
            .unwrap_err()
            .code,
        WorkspaceErrorCode::StaleRevision
    );
    validate_publication(&handle, reopened.document_id(), reopened.revision_id()).unwrap();
}

#[test]
fn full_receipt_registry_rejects_prepare_without_evicting_success() {
    let service = DocumentService::new(Default::default()).unwrap();
    let lease = open(&service);
    let mut plans = Plans::default();
    for id in 1..=RECEIPT_LIMIT {
        plans.completed.insert(
            id.to_string(),
            CleanupReceiptDto {
                plan_id: id.to_string(),
                document_id: "99".into(),
                revision: id.to_string(),
                invalidated_task_ids: vec![],
            },
        );
    }
    assert_eq!(
        run(&mut plans, &service, target(&lease), Instant::now())
            .unwrap_err()
            .code,
        WorkspaceErrorCode::ResourceLimit
    );
    assert_eq!(plans.completed.len(), RECEIPT_LIMIT);
    assert!(lease.ensure_valid().is_ok());
}
