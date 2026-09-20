//! 复用文档测试的受控加载器验证选区版本，不依赖固定休眠。

use super::*;
use crate::{documents::selection::SelectionInput, psd::LayerId};

#[test]
fn cancelled_and_failed_reloads_preserve_selection_and_prepared_commits() {
    let (service, calls) = controlled(Default::default());
    let job = service.open(fixture("viewer-scale.psd")).unwrap();
    entered(&calls).release();
    let lease = service.lease(opened(&job).0).unwrap();
    let initial = service
        .prepare_selection(
            &lease,
            service.user_selection().unwrap().revision,
            SelectionInput::Layers(vec![lease.selection_layer(LayerId(0)).unwrap()]),
        )
        .unwrap();
    service.commit_selection(initial).unwrap();
    for (id, cancel) in [(1, true), (2, false)] {
        let before = service.user_selection().unwrap();
        let prepared = service
            .prepare_selection(
                &lease,
                before.revision,
                SelectionInput::Layers(vec![lease.selection_layer(LayerId(id)).unwrap()]),
            )
            .unwrap();
        let reload = service.reload(lease.document_id()).unwrap();
        let checkpoint = entered(&calls);
        if cancel {
            assert!(service.cancel(reload.id()));
            checkpoint.release();
            assert_cancelled(&reload);
        } else {
            checkpoint.panic();
            assert!(
                matches!(&done(&reload).outcome, OpenOutcome::Failed(error) if matches!(error.as_ref(), DocumentError::WorkerPanicked))
            );
        }
        let unchanged = service.user_selection().unwrap();
        assert_eq!(unchanged.revision, before.revision);
        assert_eq!(
            unchanged.snapshot.unwrap().id(),
            before.snapshot.unwrap().id()
        );
        assert_eq!(
            service.lease(lease.document_id()).unwrap().revision_id(),
            lease.revision_id()
        );
        assert_eq!(
            service.commit_selection(prepared).unwrap().revision.get(),
            before.revision.get() + 1
        );
    }
}

#[test]
fn late_open_completions_do_not_change_selection_after_newer_or_manual_intent() {
    let (service, calls) = controlled(Default::default());
    let job = service.open(fixture("viewer-scale.psd")).unwrap();
    entered(&calls).release();
    let lease = service.lease(opened(&job).0).unwrap();
    let pending = service
        .prepare_selection(
            &lease,
            service.user_selection().unwrap().revision,
            SelectionInput::Layers(vec![lease.selection_layer(LayerId(0)).unwrap()]),
        )
        .unwrap();
    let selected = service.commit_selection(pending).unwrap();
    let old = service.open(fixture("bitmap-raw.psd")).unwrap();
    let old_checkpoint = entered(&calls);
    let newer = service.open(fixture("viewer-text.psd")).unwrap();
    old_checkpoint.release();
    opened(&old);
    let newer_checkpoint = entered(&calls);
    assert_eq!(
        service.user_selection().unwrap().revision,
        selected.revision
    );
    // 用户在较新请求尚未完成时主动选择旧标签，所有后台结果均不得抢焦点。
    service.activate(lease.document_id()).unwrap();
    newer_checkpoint.release();
    opened(&newer);
    let current = service.user_selection().unwrap();
    assert_eq!(current.revision, selected.revision);
    assert_eq!(
        current.snapshot.unwrap().id(),
        selected.snapshot.unwrap().id()
    );
    assert_eq!(current.document_id, Some(lease.document_id()));
    assert_eq!(service.snapshot().documents.len(), 3);
}
