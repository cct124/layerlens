//! 清理只使用公开入口，复用受控加载检查点验证取消与资源归还，不依赖休眠。

use super::*;
use crate::{
    documents::selection::{
        SelectionAccounting, SelectionError, SelectionInput, TaskStatus, UserSelection,
    },
    psd::LayerId,
};

fn select(service: &DocumentService, lease: &DocumentLease, id: u32) -> UserSelection {
    let prepared = service
        .prepare_selection(
            lease,
            service.user_selection().unwrap().revision,
            SelectionInput::Layers(vec![lease.selection_layer(LayerId(id)).unwrap()]),
        )
        .unwrap();
    service.commit_selection(prepared).unwrap()
}

fn assert_selection_unchanged(service: &DocumentService, before: &ServiceSnapshot) {
    let current = service.snapshot().selection;
    assert_eq!(current.revision, before.selection.revision);
    assert_eq!(current.document_id, before.selection.document_id);
    assert_eq!(
        current.document_revision,
        before.selection.document_revision
    );
    assert_eq!(current.snapshot, before.selection.snapshot);
    assert_eq!(current.items, before.selection.items);
}

#[test]
fn preflight_is_read_only_and_does_not_pin_source_resources() {
    let service = service(Default::default());
    let id = open(&service, "viewer-scale.psd");
    let before = service.snapshot();
    let plan = service.prepare_cleanup(id).unwrap();
    assert_eq!(
        plan.impact().open_revision,
        Some(before.documents[0].revision)
    );
    assert_eq!(
        plan.impact().live_revisions,
        vec![before.documents[0].revision]
    );
    assert!(plan.impact().tasks.is_empty());
    assert_eq!(service.snapshot().documents, before.documents);
    assert_selection_unchanged(&service, &before);
    assert_eq!(service.snapshot().resources, before.resources);
    service.close(id).unwrap();
    assert_eq!(service.snapshot().resources, ResourceAccounting::default());
    assert!(matches!(
        service.commit_cleanup(&plan),
        Err(DocumentError::StaleCleanupPlan)
    ));
    assert!(matches!(
        service.prepare_cleanup(id),
        Err(DocumentError::NotFound(_))
    ));
}

#[test]
fn cleanup_invalidates_reads_and_tasks_but_keeps_terminal_records_and_charges() {
    let service = service(Default::default());
    let id = open(&service, "viewer-scale.psd");
    let lease = service.lease(id).unwrap();
    let selected = select(&service, &lease, 0);
    let snapshot = selected.snapshot.as_ref().unwrap();
    let active = service
        .create_task(&selected.session_id, "active", snapshot.id(), "模块")
        .unwrap();
    let released = service
        .create_task(&selected.session_id, "released", snapshot.id(), "释放")
        .unwrap();
    service
        .release_task(&selected.session_id, released.id)
        .unwrap();
    let held = service
        .task_snapshot(&selected.session_id, active.id)
        .unwrap();
    let prepared = service
        .prepare_selection(
            &lease,
            selected.revision,
            SelectionInput::Layers(vec![lease.selection_layer(LayerId(1)).unwrap()]),
        )
        .unwrap();
    let plan = service.prepare_cleanup(id).unwrap();
    assert_eq!(plan.impact().tasks, vec![active.clone()]);
    assert_eq!(plan.impact().selection, Some(snapshot.id()));
    assert!(!service.commit_cleanup(&plan).unwrap().already_applied);
    let current = service.user_selection().unwrap();
    assert_eq!(current.revision.get(), selected.revision.get() + 1);
    assert!(current.document_id.is_none());
    assert!(current.snapshot.is_none());
    assert!(matches!(
        service.commit_selection(prepared),
        Err(SelectionError::Document(DocumentError::Invalidated))
    ));
    assert!(matches!(
        lease.layers(0, 1),
        Err(DocumentError::Invalidated)
    ));
    assert!(matches!(
        lease.layer_details(LayerId(0), 0),
        Err(DocumentError::Invalidated)
    ));
    assert!(matches!(
        lease.selection_layer(LayerId(0)),
        Err(DocumentError::Invalidated)
    ));
    assert!(matches!(
        held.content_page(None, 1),
        Err(SelectionError::Document(DocumentError::Invalidated))
    ));
    assert!(matches!(
        service.selection_snapshot(&selected.session_id, snapshot.id()),
        Err(SelectionError::SnapshotExpired)
    ));
    assert!(matches!(
        service.task_snapshot(&selected.session_id, active.id),
        Err(SelectionError::TaskInvalidated)
    ));
    assert!(matches!(
        service.create_task(&selected.session_id, "new", snapshot.id(), "新任务"),
        Err(SelectionError::SnapshotExpired)
    ));
    let retry = service
        .create_task(&selected.session_id, "active", snapshot.id(), "模块")
        .unwrap();
    assert_eq!(retry.status, TaskStatus::Invalidated);
    assert_eq!(
        service
            .release_task(&selected.session_id, active.id)
            .unwrap(),
        retry
    );
    let page = service
        .selection_handle()
        .list_tasks(&selected.session_id, None, 32)
        .unwrap();
    assert_eq!(page.total, 2);
    assert_eq!(page.tasks[1].status, TaskStatus::Released);
    assert_eq!(service.selection_resources().live_snapshots, 1);
    assert_eq!(service.snapshot().resources.live_revisions, 1);
    drop((lease, selected, held));
    assert_eq!(
        service.selection_resources(),
        SelectionAccounting::default()
    );
    assert_eq!(service.snapshot().resources, ResourceAccounting::default());
    assert!(service.commit_cleanup(&plan).unwrap().already_applied);
    assert_eq!(service.user_selection().unwrap().revision, current.revision);
}

#[test]
fn all_retained_revisions_are_invalidated_without_touching_other_document() {
    let service = service(Default::default());
    let id = open(&service, "viewer-scale.psd");
    let old = service.lease(id).unwrap();
    let selected = select(&service, &old, 0);
    let task = service
        .create_task(
            &selected.session_id,
            "old",
            selected.snapshot.as_ref().unwrap().id(),
            "旧修订",
        )
        .unwrap();
    opened(&service.reload(id).unwrap());
    let new = service.lease(id).unwrap();
    select(&service, &new, 1);
    let other_id = open(&service, "viewer-text.psd");
    let other = service.lease(other_id).unwrap();
    let other_selected = select(&service, &other, 0);
    let other_task = service
        .create_task(
            &selected.session_id,
            "other",
            other_selected.snapshot.as_ref().unwrap().id(),
            "其他文档",
        )
        .unwrap();
    let plan = service.prepare_cleanup(id).unwrap();
    assert_eq!(
        plan.impact().live_revisions,
        vec![old.revision_id(), new.revision_id()]
    );
    service.commit_cleanup(&plan).unwrap();
    assert!(matches!(
        old.ensure_valid(),
        Err(DocumentError::Invalidated)
    ));
    assert!(matches!(
        new.ensure_valid(),
        Err(DocumentError::Invalidated)
    ));
    assert_eq!(
        service
            .design_task(&selected.session_id, task.id)
            .unwrap()
            .status,
        TaskStatus::Invalidated
    );
    assert_eq!(
        service.user_selection().unwrap().revision,
        other_selected.revision
    );
    assert_eq!(service.snapshot().active_document, Some(other_id));
    assert_eq!(
        service
            .task_snapshot(&selected.session_id, other_task.id)
            .unwrap()
            .id(),
        other_selected.snapshot.unwrap().id()
    );
    assert!(other.layers(0, 1).is_ok());
}

#[test]
fn closed_document_cleanup_and_retry_leave_reopened_path_and_reload_alone() {
    let (service, calls) = controlled(Default::default());
    let job = service.open(fixture("viewer-scale.psd")).unwrap();
    entered(&calls).release();
    let old = service.lease(opened(&job).0).unwrap();
    let selected = select(&service, &old, 0);
    let task = service
        .create_task(
            &selected.session_id,
            "old",
            selected.snapshot.as_ref().unwrap().id(),
            "旧任务",
        )
        .unwrap();
    service.close(old.document_id()).unwrap();
    let job = service.open(fixture("viewer-scale.psd")).unwrap();
    entered(&calls).release();
    let new_id = opened(&job).0;
    assert_ne!(new_id, old.document_id());
    let reload = service.reload(new_id).unwrap();
    let checkpoint = entered(&calls);
    let plan = service.prepare_cleanup(old.document_id()).unwrap();
    assert!(plan.impact().open_revision.is_none());
    assert!(plan.impact().load_jobs.is_empty());
    let revision = service.user_selection().unwrap().revision;
    service.commit_cleanup(&plan).unwrap();
    assert_eq!(service.user_selection().unwrap().revision, revision);
    assert!(matches!(reload.status(), JobStatus::Running));
    checkpoint.release();
    assert_eq!(opened(&reload).0, new_id);
    let revision = service.user_selection().unwrap().revision;
    assert!(service.commit_cleanup(&plan).unwrap().already_applied);
    assert_eq!(service.user_selection().unwrap().revision, revision);
    assert!(service.lease(new_id).unwrap().ensure_valid().is_ok());
    assert!(matches!(
        service.task_snapshot(&selected.session_id, task.id),
        Err(SelectionError::TaskInvalidated)
    ));
}

#[test]
fn applied_open_document_plan_does_not_close_later_reopening() {
    let service = service(Default::default());
    let id = open(&service, "bitmap-raw.psd");
    let plan = service.prepare_cleanup(id).unwrap();
    service.commit_cleanup(&plan).unwrap();
    let reopened = open(&service, "bitmap-raw.psd");
    assert_ne!(id, reopened);
    let before = service.snapshot();
    assert!(service.commit_cleanup(&plan).unwrap().already_applied);
    assert_eq!(service.snapshot().documents, before.documents);
    assert_selection_unchanged(&service, &before);
}

#[test]
fn changed_selection_tasks_or_revision_require_fresh_confirmation_without_side_effects() {
    for change in ["selection", "create", "release", "reload"] {
        let service = service(Default::default());
        let id = open(&service, "viewer-scale.psd");
        let lease = service.lease(id).unwrap();
        let selected = select(&service, &lease, 0);
        let task = service
            .create_task(
                &selected.session_id,
                "initial",
                selected.snapshot.as_ref().unwrap().id(),
                "模块",
            )
            .unwrap();
        let plan = service.prepare_cleanup(id).unwrap();
        match change {
            "selection" => {
                select(&service, &lease, 1);
            }
            "create" => {
                service
                    .create_task(&selected.session_id, "new", task.snapshot_id, "新任务")
                    .unwrap();
            }
            "release" => {
                service.release_task(&selected.session_id, task.id).unwrap();
            }
            "reload" => {
                opened(&service.reload(id).unwrap());
            }
            _ => unreachable!(),
        }
        let before = service.snapshot();
        let tasks = service
            .selection_handle()
            .list_tasks(&selected.session_id, None, 32)
            .unwrap()
            .tasks;
        assert!(
            matches!(
                service.commit_cleanup(&plan),
                Err(DocumentError::StaleCleanupPlan)
            ),
            "{change}"
        );
        assert_eq!(service.snapshot().documents, before.documents);
        assert_selection_unchanged(&service, &before);
        assert_eq!(
            service
                .selection_handle()
                .list_tasks(&selected.session_id, None, 32)
                .unwrap()
                .tasks,
            tasks
        );
        assert!(lease.ensure_valid().is_ok());
    }
}

#[test]
fn foreign_plan_is_rejected_even_when_numeric_ids_match() {
    let a = service(Default::default());
    let b = service(Default::default());
    let id = open(&a, "bitmap-raw.psd");
    assert_eq!(id, open(&b, "bitmap-raw.psd"));
    let plan = a.prepare_cleanup(id).unwrap();
    assert!(matches!(
        b.commit_cleanup(&plan),
        Err(DocumentError::ForeignCleanupPlan)
    ));
    assert!(b.lease(id).unwrap().ensure_valid().is_ok());
    assert!(!a.commit_cleanup(&plan).unwrap().already_applied);
}

#[test]
fn changed_load_set_rejects_then_fresh_cleanup_cancels_running_and_queued_aliases() {
    let (service, calls) = controlled(Default::default());
    let job = service.open(fixture("bitmap-raw.psd")).unwrap();
    entered(&calls).release();
    let id = opened(&job).0;
    let stale = service.prepare_cleanup(id).unwrap();
    let reload = service.reload(id).unwrap();
    let checkpoint = entered(&calls);
    let before_alias = service.prepare_cleanup(id).unwrap();
    // Windows absolute 折叠 ..；大小写别名直到后台 canonicalize 才能识别。
    let path = if cfg!(windows) {
        "BITMAP-RAW.PSD"
    } else {
        "../psd/bitmap-raw.psd"
    };
    let alias = service.open(fixture(path)).unwrap();
    let unrelated = service.open(fixture("bitmap-rle.psd")).unwrap();
    assert!(matches!(
        service.commit_cleanup(&stale),
        Err(DocumentError::StaleCleanupPlan)
    ));
    assert!(matches!(
        service.commit_cleanup(&before_alias),
        Err(DocumentError::StaleCleanupPlan)
    ));
    assert!(matches!(reload.status(), JobStatus::Running));
    assert!(matches!(alias.status(), JobStatus::Queued));
    let plan = service.prepare_cleanup(id).unwrap();
    assert_eq!(plan.impact().load_jobs, vec![reload.id()]);
    assert_eq!(
        plan.impact().unresolved_load_jobs,
        vec![alias.id(), unrelated.id()]
    );
    service.commit_cleanup(&plan).unwrap();
    assert!(matches!(reload.status(), JobStatus::Cancelling));
    assert!(service.snapshot().resources.live_revisions > 0);
    checkpoint.release();
    assert_cancelled(&reload);
    assert_cancelled(&alias);
    let other_checkpoint = entered(&calls);
    assert!(service.snapshot().documents.is_empty());
    other_checkpoint.release();
    let other = opened(&unrelated).0;
    assert_eq!(service.snapshot().documents.len(), 1);
    assert_eq!(service.snapshot().resources.live_revisions, 1);
    service.close(other).unwrap();
    assert_eq!(service.snapshot().resources, ResourceAccounting::default());
    assert!(calls.try_recv().is_err());
}

#[test]
fn cleanup_cancels_queued_reload_while_prior_cancelled_parser_is_still_charged() {
    let (service, calls) = controlled(Default::default());
    let job = service.open(fixture("bitmap-raw.psd")).unwrap();
    entered(&calls).release();
    let id = opened(&job).0;
    let running = service.reload(id).unwrap();
    let checkpoint = entered(&calls);
    let queued = service.reload(id).unwrap();
    let plan = service.prepare_cleanup(id).unwrap();
    assert_eq!(plan.impact().load_jobs, vec![running.id(), queued.id()]);
    service.commit_cleanup(&plan).unwrap();
    assert_cancelled(&queued);
    assert!(matches!(running.status(), JobStatus::Cancelling));
    assert_eq!(service.snapshot().pending_jobs.len(), 1);
    checkpoint.release();
    assert_cancelled(&running);
    assert_eq!(service.snapshot().resources, ResourceAccounting::default());
    assert!(calls.try_recv().is_err());
}

#[test]
fn concurrent_confirmation_of_same_plan_applies_exactly_once() {
    let service = service(Default::default());
    let id = open(&service, "bitmap-raw.psd");
    let before = service.user_selection().unwrap().revision;
    let plan = service.prepare_cleanup(id).unwrap();
    let other = service.prepare_cleanup(id).unwrap();
    std::thread::scope(|scope| {
        let first = scope.spawn(|| service.commit_cleanup(&plan).unwrap());
        let second = scope.spawn(|| service.commit_cleanup(&plan).unwrap());
        assert_ne!(
            first.join().unwrap().already_applied,
            second.join().unwrap().already_applied
        );
    });
    assert_eq!(
        service.user_selection().unwrap().revision.get(),
        before.get() + 1
    );
    assert!(matches!(
        service.commit_cleanup(&other),
        Err(DocumentError::StaleCleanupPlan)
    ));
}

#[test]
fn stopping_service_rejects_unapplied_cleanup_without_invalidating_external_lease() {
    let mut service = service(Default::default());
    let id = open(&service, "bitmap-raw.psd");
    let lease = service.lease(id).unwrap();
    let plan = service.prepare_cleanup(id).unwrap();
    service.shutdown().unwrap();
    assert!(matches!(
        service.prepare_cleanup(id),
        Err(DocumentError::ShuttingDown)
    ));
    assert!(matches!(
        service.commit_cleanup(&plan),
        Err(DocumentError::ShuttingDown)
    ));
    assert!(lease.ensure_valid().is_ok());
}
