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
            syncing: false,
            sync_error: false,
            pending: false,
            rendered_tasks: 0,
            scrollable: false,
            scrolled_to_end: false,
            focus_inside: false,
        },
        canvas: None,
        tasks: vec![],
        selection_running: false,
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
        "task:more",
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
fn canvas_observation() -> UiObservation {
    let mut view = observation(100, 1);
    view.canvas = Some(UiCanvas {
        viewport: UiRect {
            x: 10.0,
            y: 20.0,
            width: 100.0,
            height: 80.0,
        },
        image: None,
        document_width: 256,
        document_height: 128,
        image_decoded: true,
        paint_prepared: true,
        image_error: None,
        zoom: 1.0,
        pan_x: 0.0,
        pan_y: 0.0,
        panning: false,
        pointer_captured: false,
        draft: None,
        alt: false,
        snap_guides: 0,
        snap_x: None,
        snap_y: None,
    });
    view
}
#[test]
fn scroll_rejects_outside_modal_unready_and_unbounded_input() {
    assert!(super::driver::validate_scroll(&observation(100, 1), 10, 20, 1).is_err());
    let mut view = canvas_observation();
    for delta in [-200, -1, 1, 200] {
        assert!(super::driver::validate_scroll(&view, 10, 20, delta).is_ok());
    }
    for (x, y, delta) in [
        (9, 20, 1),
        (110, 20, 1),
        (10, 100, 1),
        (10, 20, 0),
        (10, 20, i32::MIN),
        (10, 20, 201),
    ] {
        assert_eq!(
            super::driver::validate_scroll(&view, x, y, delta)
                .unwrap_err()
                .code,
            Code::InvalidInput
        );
    }
    view.cleanup.opened = true;
    assert!(super::driver::validate_scroll(&view, 10, 20, 1).is_err());
    view.cleanup.opened = false;
    view.canvas.as_mut().unwrap().image_decoded = false;
    assert!(super::driver::validate_scroll(&view, 10, 20, 1).is_err());
}

#[test]
fn bounded_gesture_validates_every_step_before_dispatch_and_keeps_button_order() {
    use GestureStep::*;
    use PointerButton::{Left, Middle};
    let view = canvas_observation();
    let start = Move { x: 20, y: 30 };
    let chord = vec![
        start.clone(),
        Down { button: Left },
        Move { x: 40, y: 40 },
        Down { button: Middle },
        Move { x: 60, y: 50 },
        Up { button: Middle },
        Move { x: 70, y: 50 },
        Up { button: Left },
    ];
    let actions = super::gesture::actions(&view, &chord, false).unwrap();
    let pointer = actions["actions"][0]["actions"].as_array().unwrap();
    assert_eq!(pointer.len(), chord.len());
    assert_eq!(
        pointer[3],
        serde_json::json!({"type":"pointerDown","button":1})
    );
    assert_eq!(
        pointer[5],
        serde_json::json!({"type":"pointerUp","button":1})
    );
    for invalid in [
        vec![],
        vec![Down { button: Left }],
        vec![start.clone()],
        vec![start.clone(), Up { button: Left }],
        vec![start.clone(), Down { button: Left }, Down { button: Left }],
        vec![start.clone(), Down { button: Middle }, Up { button: Left }],
        vec![
            start.clone(),
            Down { button: Left },
            Move { x: 110, y: 30 },
            Up { button: Left },
        ],
        vec![start.clone(); 33],
        vec![start.clone(), Escape],
    ] {
        assert_eq!(
            super::gesture::actions(&view, &invalid, false)
                .unwrap_err()
                .code,
            Code::InvalidInput
        );
    }
    let held = [
        start.clone(),
        Down { button: Middle },
        Move { x: 50, y: 40 },
    ];
    assert!(super::gesture::actions(&view, &held, true).is_ok());
    assert!(super::gesture::actions(&view, &held, false).is_err());
    assert!(super::gesture::actions(&view, &chord, true).is_err());
    let mut modal = view.clone();
    modal.cleanup.opened = true;
    assert!(super::gesture::actions(&modal, &chord, false).is_err());
    let cancelled = [start, Down { button: Left }, Escape, Up { button: Left }];
    let actions = super::gesture::actions(&view, &cancelled, false).unwrap();
    assert_eq!(
        actions["actions"][0]["actions"].as_array().unwrap().len(),
        5
    );
    assert_eq!(actions["actions"][1]["actions"][2]["type"], "keyDown");
    assert_eq!(actions["actions"][1]["actions"][3]["type"], "keyUp");
    assert!(
        serde_json::from_value::<UiAction>(serde_json::json!({
            "kind":"canvasGesture","hold":false,"steps":[{"kind":"down","button":"right"}]
        }))
        .is_err()
    );
}

#[test]
fn canvas_observation_rejects_invalid_pan_and_inconsistent_snap_evidence() {
    let state = Observations::default();
    let mut view = canvas_observation();
    view.canvas.as_mut().unwrap().pan_x = f64::INFINITY;
    assert!(state.publish("instance", view).is_err());
    let mut view = canvas_observation();
    view.canvas.as_mut().unwrap().snap_guides = 1;
    assert!(state.publish("instance", view).is_err());
    let mut view = canvas_observation();
    let canvas = view.canvas.as_mut().unwrap();
    canvas.snap_guides = 1;
    canvas.snap_x = Some(UiSnapTarget {
        position: 0.0,
        layer_id: None,
    });
    state.publish("instance", view).unwrap();
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
