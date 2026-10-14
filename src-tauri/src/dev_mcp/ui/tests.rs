//! 观测与输入边界的确定性回归；真实驱动证据由桌面脚本另行记录。
use super::{model::*, observation::Observations, targets};
use layerlens_core::workspace_contract::WorkspaceErrorCode as Code;
use tokio_util::sync::CancellationToken;

fn observation(epoch: u64, revision: u64) -> UiObservation {
    UiObservation {
        instance_id: "instance".into(),
        frontend_id: "11111111-1111-4111-8111-111111111111".into(),
        document_epoch: epoch.to_string(),
        view_revision: revision.to_string(),
        core_sequence: Some(revision.to_string()),
        session_id: Some("session".into()),
        active_document: None,
        selection_revision: Some("1".into()),
        snapshot_id: None,
        region: None,
        tool: "pan".into(),
        panel: "layers".into(),
        menu: None,
        focus: None,
        cleanup: UiCleanup {
            opened: false,
            phase: None,
            plan_id: None,
            task_ids: vec![],
        },
        canvas: None,
        tasks: vec![],
        content: None,
        input: UiInputEvidence {
            pointer_events: "0".into(),
            key_events: "0".into(),
            last_key: None,
        },
    }
}
#[test]
fn old_pages_out_of_order_reports_and_foreign_instances_cannot_replace_observation() {
    let state = Observations::default();
    state.publish("instance", observation(100, 2)).unwrap();
    assert_eq!(
        state
            .publish("other", observation(100, 3))
            .unwrap_err()
            .code,
        Code::ForeignSession
    );
    assert_eq!(
        state
            .publish("instance", observation(100, 1))
            .unwrap_err()
            .code,
        Code::StaleRevision
    );
    assert_eq!(state.read().unwrap().view_revision, "2");
    state.invalidate();
    assert_eq!(state.read().unwrap_err().code, Code::NotFound);
    assert_eq!(
        state
            .publish("instance", observation(100, 3))
            .unwrap_err()
            .code,
        Code::StaleRevision
    );
    let mut new = observation(101, 1);
    new.frontend_id = "22222222-2222-4222-8222-222222222222".into();
    state.publish("instance", new.clone()).unwrap();
    assert_eq!(
        state
            .publish("instance", observation(100, 4))
            .unwrap_err()
            .code,
        Code::StaleRevision
    );
    assert_eq!(state.read().unwrap(), new);
}
#[test]
fn observation_size_numbers_and_semantic_targets_are_validated() {
    let state = Observations::default();
    let mut oversized = observation(100, 1);
    oversized.focus = Some("x".repeat(40 * 1024));
    assert_eq!(
        state.publish("instance", oversized).unwrap_err().code,
        Code::ResourceLimit
    );
    let mut invalid = observation(100, 1);
    invalid.core_sequence = Some("01".into());
    assert_eq!(
        state.publish("instance", invalid).unwrap_err().code,
        Code::InvalidInput
    );
    for valid in [
        "menu:file",
        "command:tasks",
        "task:1:view",
        "document:2",
        "layer:0:check",
        "handle:se",
    ] {
        assert!(targets::selector(valid).is_ok());
    }
    for invalid in [
        "body",
        "document:01",
        "task:1:delete",
        "task:1:view,body",
        "handle:script",
        "command:open",
    ] {
        assert!(targets::selector(invalid).is_err());
    }
    assert!(
        serde_json::from_value::<UiAction>(
            serde_json::json!({"kind":"click","target":"canvas","script":"bad"})
        )
        .is_err()
    );
}
#[tokio::test]
async fn wait_wakes_on_actual_report_times_out_with_last_state_and_rejects_generation_change() {
    let state = Observations::default();
    let stop = CancellationToken::new();
    state.publish("instance", observation(100, 1)).unwrap();
    let waiting = state.wait(
        UiWaitRequest {
            frontend_id: Some(observation(100, 1).frontend_id),
            condition: UiCondition::Applied {
                at_least: "2".into(),
            },
            timeout_ms: 5000,
        },
        &stop,
    );
    tokio::pin!(waiting);
    tokio::select! { biased; _=&mut waiting => panic!("不能提前报告已同步"), _=tokio::task::yield_now()=>{} }
    state.publish("instance", observation(100, 2)).unwrap();
    let ready = waiting.await.unwrap();
    assert!(ready.satisfied);
    assert_eq!(
        ready.last_state.unwrap().core_sequence.as_deref(),
        Some("2")
    );
    let timeout = state
        .wait(
            UiWaitRequest {
                frontend_id: None,
                condition: UiCondition::Applied {
                    at_least: "3".into(),
                },
                timeout_ms: 1,
            },
            &stop,
        )
        .await
        .unwrap();
    assert!(timeout.timed_out);
    assert_eq!(timeout.last_state.unwrap().view_revision, "2");
    let stale = state
        .wait(
            UiWaitRequest {
                frontend_id: Some("old".into()),
                condition: UiCondition::Ready {},
                timeout_ms: 10,
            },
            &stop,
        )
        .await
        .unwrap_err();
    assert_eq!(stale.code, Code::StaleRevision);
    stop.cancel();
    assert_eq!(
        state
            .wait(
                UiWaitRequest {
                    frontend_id: None,
                    condition: UiCondition::Applied {
                        at_least: "3".into()
                    },
                    timeout_ms: 1000
                },
                &stop
            )
            .await
            .unwrap_err()
            .code,
        Code::ShuttingDown
    );
}
